//! Static, bounded interpretation of box's quoted import syntax. This never
//! loads a module or a package: only parsed project buffers, local module
//! source, installed NAMESPACE declarations, and typeshed are consulted.

use crate::scope_journal::BranchDelta;
use crate::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

const MAX_MODULE_BYTES: u64 = 1_048_576;
const MAX_MODULE_DEPTH: u8 = 4;
/// Function-alias prefix of a callable imported from a package.
const IMPORT_ALIAS: &str = "__ry_box_import::";

/// Project buffers by physical identity. `None` marks an open buffer that
/// this check does not read, whose disk copy may be stale.
pub(crate) type BoxSources = HashMap<PathBuf, Option<Arc<SourceFile>>>;

/// Box provenance of a binding, stored beside its value type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoxObject {
    Package(String),
    Module(Arc<BoxInventory>),
    /// A selected or wildcard attachment, with its call target if known.
    Attached(Option<BoxCallable>),
    /// A binding whose call target is unknown: an own non-function value
    /// hiding an attachment, or provenance that differs between branches.
    OpaqueCall,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoxFunction {
    params: Vec<UserParam>,
    return_type: RType,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoxCallable {
    Function(Arc<BoxFunction>),
    /// A package function, by its qualified `pkg::name`.
    Package(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoxExport {
    pub(crate) value: RType,
    pub(crate) callable: Option<BoxCallable>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct BoxInventory {
    pub(crate) exports: BTreeMap<String, BoxExport>,
    /// False if a dynamic export declaration or unmodeled source effect may
    /// add names; absence then cannot support RY118.
    pub(crate) complete: bool,
}

#[derive(Debug)]
enum Target {
    Package(String),
    Module(Vec<String>),
    UnresolvedModule,
}

#[derive(Debug)]
struct Selection {
    original: String,
    bound: String,
    span: Span,
}

#[derive(Debug)]
struct Import {
    target: Target,
    object_name: Option<String>,
    selection: Option<Vec<Selection>>,
    wildcard: bool,
    selection_unknown: bool,
}

pub(crate) fn binding_name_token(raw: &str) -> Option<String> {
    if matches!(raw.as_bytes().first(), Some(b'`' | b'\'' | b'"')) {
        ry_core::parser::decode_r_quoted_name(raw)
    } else {
        Some(raw.to_string())
    }
}

fn static_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident { name, .. } => binding_name_token(name),
        // String expressions have already been decoded by the parser.
        Expr::String(name, _) => Some(name.clone()),
        _ => None,
    }
}

fn path_segments(expr: &Expr, segments: &mut Vec<String>) -> bool {
    match expr {
        Expr::Ident { name, .. } => binding_name_token(name)
            .map(|name| segments.push(name))
            .is_some(),
        Expr::BinOp {
            op: BinOpKind::Div,
            lhs,
            rhs,
            ..
        } => path_segments(lhs, segments) && path_segments(rhs, segments),
        _ => false,
    }
}

/// Collect an import's path and return its `[...]` selection, if any. The
/// parser attaches an index to the final path component (`./a/b[x]` is
/// `./a/(b[x])`); other index kinds there select nothing.
fn import_path<'a>(expr: &'a Expr, segments: &mut Vec<String>) -> Option<Option<&'a [Arg]>> {
    match expr {
        Expr::Index {
            base, kind, args, ..
        } => path_segments(base, segments)
            .then_some((*kind == IndexKind::Single).then_some(args.as_slice())),
        Expr::BinOp {
            op: BinOpKind::Div,
            lhs,
            rhs,
            ..
        } if matches!(rhs.as_ref(), Expr::Index { .. }) => {
            if path_segments(lhs, segments) {
                import_path(rhs, segments)
            } else {
                None
            }
        }
        _ => path_segments(expr, segments).then_some(None),
    }
}

fn parse_import(argument: &Arg) -> Option<Import> {
    let mut segments = Vec::new();
    let selected = import_path(&argument.value, &mut segments)?;
    let mut wildcard = false;
    let mut selection_unknown = false;
    let selection = selected.map(|args| {
        let mut picked = Vec::new();
        for arg in args {
            let original = static_name(&arg.value);
            if original.as_deref() == Some("...") && arg.name.is_none() {
                wildcard = true;
                continue;
            }
            let bound = match &arg.name {
                Some(alias) => binding_name_token(alias),
                None => original.clone(),
            };
            match (original, bound) {
                (Some(original), Some(bound)) if original != "..." => picked.push(Selection {
                    original,
                    bound,
                    span: arg.span,
                }),
                _ => selection_unknown = true,
            }
        }
        picked
    });
    // A selective import binds only the selected names unless the import
    // explicitly names the module/package object.
    let object_name = match &argument.name {
        Some(alias) => Some(binding_name_token(alias)?),
        None if selection.is_none() => segments.last().cloned(),
        None => None,
    };
    let target = match segments.as_slice() {
        [package] if package != "." && package != ".." => Target::Package(package.clone()),
        [first, ..] if first == "." || first == ".." => Target::Module(segments),
        // Non-relative modules use box's configured search path. We do not
        // model that lookup, but their static aliases and selections still
        // bind opaque values through the ordinary import path.
        _ => Target::UnresolvedModule,
    };
    Some(Import {
        target,
        object_name,
        selection,
        wildcard,
        selection_unknown,
    })
}

/// Project invalidation needs to know whether an unchanged file may import
/// an edited module. Search parsed calls, not arbitrary `box` text in
/// comments, identifiers such as `boxplot`, or string literals.
pub(crate) fn has_box_use(file: &SourceFile) -> bool {
    if !file.source.contains("box") {
        return false;
    }
    walk_stmts(&file.stmts, Walk::ALL, |node, _| {
        if matches!(node, AstNode::Expr(Expr::Call { func, .. }) if ident_name(func) == Some("box::use")) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(Descend::Into)
        }
    })
    .is_break()
}

/// An existing file's physical identity, or the physical identity of its
/// nearest existing ancestor plus an unsaved final path. This lets a module
/// opened in an editor match its on-disk caller even with a symlinked root.
pub(crate) fn path_identity(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    let mut existing = absolute.as_path();
    let mut tail = Vec::new();
    while !existing.exists() {
        tail.push(existing.file_name()?.to_os_string());
        existing = existing.parent()?;
    }
    let mut resolved = std::fs::canonicalize(existing).ok()?;
    for component in tail.into_iter().rev() {
        resolved.push(component);
    }
    Some(resolved)
}

fn module_candidates(caller: &Path, segments: &[String]) -> Option<[PathBuf; 4]> {
    let mut stem = caller.parent()?.to_path_buf();
    for segment in segments {
        if !matches!(segment.as_str(), "." | "..")
            && (segment.is_empty() || segment.contains(['/', '\\']))
        {
            return None;
        }
        stem.push(segment);
    }
    // `with_extension` replaces the suffix of `./foo.bar`, but box appends
    // `.r` to the entire module name and loads `foo.bar.r`.
    let suffixed = |extension: &str| {
        let mut path = stem.as_os_str().to_os_string();
        path.push(extension);
        PathBuf::from(path)
    };
    Some([
        suffixed(".r"),
        suffixed(".R"),
        stem.join("__init__.r"),
        stem.join("__init__.R"),
    ])
}

/// The collector indexes literal functions by spelling. A later wrapper
/// assignment can leave the final value callable while the indexed literal
/// is no longer its definition. Retain the collected signature only for a
/// single direct literal binding with no other module-load assignment to the
/// same name. Control-flow bindings are counted; nested function bodies have
/// their own lexical scope and are excluded.
fn stable_direct_function_bindings(file: &SourceFile, load: &ModuleLoad) -> HashSet<String> {
    if load.unmodeled_effects {
        return HashSet::new();
    }
    file.stmts
        .iter()
        .filter_map(|statement| match statement {
            Stmt::Assign {
                target,
                value: Expr::Function { .. },
                ..
            } => binding_name(target),
            _ => None,
        })
        .map(|name| infer::semantic_argument_name(name).to_string())
        .filter(|name| load.counts.get(name) == Some(&1))
        .collect()
}

/// What loading a module does outside function bodies, which are inert
/// until called. Assignment targets are keyed by semantic name.
#[derive(Default)]
struct ModuleLoad {
    counts: HashMap<String, usize>,
    /// Targets of assignment expressions, which also cover `<<-`/`%<>%`.
    expression_names: HashSet<String>,
    /// Targets of `<-`/`=` expressions, which bind in the module itself.
    own_expression_names: HashSet<String>,
    /// Calls may create caller-frame bindings, including through qualified
    /// writers and sourced files. We do not execute them, so a missing
    /// syntactic assignment is not proof that a name is absent.
    unmodeled_effects: bool,
    /// box's quoted imports change the lexical environment of functions.
    imports: bool,
}

fn module_load(file: &SourceFile) -> ModuleLoad {
    let mut load = ModuleLoad::default();
    let walk = Walk {
        fn_bodies: false,
        ..Walk::ALL
    };
    let _ = walk_stmts(&file.stmts, walk, |node, _| {
        let (target, operator) = match node {
            AstNode::Stmt(Stmt::Assign { target, .. }) => (Some(target), None),
            AstNode::Expr(Expr::BinOp { op, lhs, .. })
                if matches!(
                    op,
                    BinOpKind::Assign | BinOpKind::SuperAssign | BinOpKind::PipeAssign
                ) =>
            {
                (Some(lhs.as_ref()), Some(*op))
            }
            _ => (None, None),
        };
        if let Some(name) = target.and_then(binding_name) {
            let name = infer::semantic_argument_name(name).to_string();
            if let Some(op) = operator {
                if op == BinOpKind::Assign {
                    load.own_expression_names.insert(name.clone());
                }
                load.expression_names.insert(name.clone());
            }
            *load.counts.entry(name).or_default() += 1;
        }
        let AstNode::Expr(expr) = node else {
            return ControlFlow::<(), Descend>::Continue(Descend::Into);
        };
        let descend = match expr {
            Expr::Call { func, args, .. } if ident_name(func) == Some("box::use") => {
                load.imports = true;
                load.unmodeled_effects |=
                    args.iter().any(|argument| parse_import(argument).is_none());
                Descend::Skip
            }
            Expr::Call { func, args, .. }
                if ident_name(func) == Some("box::export")
                    && args
                        .iter()
                        .all(|argument| static_name(&argument.value).is_some()) =>
            {
                Descend::Skip
            }
            // `%<>%` is dispatched through an operator, and executed braced
            // or conditional expressions can write bindings without a
            // top-level Stmt::Assign.
            Expr::Call { .. }
            | Expr::BinOp {
                op: BinOpKind::PipeAssign,
                ..
            }
            | Expr::Block { .. }
            | Expr::If { .. } => {
                load.unmodeled_effects = true;
                Descend::Into
            }
            _ => Descend::Into,
        };
        ControlFlow::Continue(descend)
    });
    load
}

struct RoxygenExports {
    names: HashSet<String>,
    tagged: bool,
    complete: bool,
}

fn has_export_tag(lines: &[&str]) -> bool {
    for line in lines {
        let text = line.trim_start_matches([' ', '\t']);
        if text.starts_with('#') {
            // box 1.2.3's scanner requires the tag to end the line; even
            // trailing spaces make it an ordinary comment.
            if text
                .trim_start_matches('#')
                .strip_prefix('\'')
                .is_some_and(|tag| tag.trim_start_matches([' ', '\t']) == "@export")
            {
                return true;
            }
        } else if !text.is_empty() {
            return false;
        }
    }
    false
}

fn top_level_bindings(file: &SourceFile) -> (HashSet<String>, RoxygenExports) {
    let mut assigned = HashSet::new();
    let mut roxygen = RoxygenExports {
        names: HashSet::new(),
        tagged: false,
        complete: true,
    };
    let lines: Vec<&str> = file.source.lines().collect();
    let mut region_start = 0;
    for statement in &file.stmts {
        let span = infer::vacuous::stmt_span(statement);
        // box begins each comment region on the line after the preceding
        // expression ends. Blank lines and ordinary comments do not end it.
        let tagged = has_export_tag(lines.get(region_start..span.line).unwrap_or_default());
        region_start = span.line
            + file.source[span.start..span.end]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
            + 1;
        roxygen.tagged |= tagged;
        if let Stmt::Expr(Expr::Call { func, args, .. }) = statement
            && ident_name(func) == Some("box::use")
        {
            for argument in args {
                let Some(import) = parse_import(argument) else {
                    roxygen.complete &= !tagged;
                    continue;
                };
                // Plain and explicitly aliased imports bind in the module
                // namespace. A tag also exports that object and the
                // attached aliases; a wildcard may add unenumerated names.
                if let Some(name) = import.object_name {
                    if tagged {
                        roxygen.names.insert(name.clone());
                    }
                    assigned.insert(name);
                }
                if tagged {
                    roxygen.names.extend(
                        import
                            .selection
                            .into_iter()
                            .flatten()
                            .map(|selection| selection.bound),
                    );
                    roxygen.complete &= !import.wildcard && !import.selection_unknown;
                }
            }
            continue;
        }
        let Stmt::Assign {
            target,
            value,
            span,
        } = statement
        else {
            roxygen.complete &= !tagged;
            continue;
        };
        // The parser wraps a direct `foo <<- value` or `value ->> foo`
        // statement in Stmt::Assign with a SuperAssign expression spanning
        // the whole statement. It writes through the module environment,
        // not to an own binding, and box rejects a tag on it at load time.
        // An ordinary `foo <- (bar <<- value)` has a smaller inner span.
        let direct_superassignment = matches!(value, Expr::BinOp { op: BinOpKind::SuperAssign, lhs, span: marker, .. }
            if marker == span && binding_name(lhs) == binding_name(target));
        let name = binding_name(target).filter(|_| !direct_superassignment);
        let Some(name) = name else {
            roxygen.complete &= !tagged;
            continue;
        };
        let name = infer::semantic_argument_name(name).to_string();
        if tagged {
            roxygen.names.insert(name.clone());
        }
        assigned.insert(name);
    }
    (assigned, roxygen)
}

fn declared_exports(
    file: &SourceFile,
    mut assigned: HashSet<String>,
    roxygen: RoxygenExports,
    unmodeled_load_effects: bool,
) -> (HashSet<String>, bool) {
    let mut names = HashSet::new();
    let mut complete = !unmodeled_load_effects;
    let mut direct_calls = 0;
    for statement in &file.stmts {
        if let Stmt::Expr(Expr::Call { func, args, .. }) = statement
            && ident_name(func) == Some("box::export")
        {
            direct_calls += 1;
            for argument in args {
                match static_name(&argument.value) {
                    Some(name) => {
                        names.insert(name);
                    }
                    None => complete = false,
                }
            }
        }
    }
    // A braced, conditional, or otherwise nested export call may add names
    // at runtime. It also prevents legacy fallback, even when it declares
    // no statically readable name.
    let mut all_calls = 0;
    let _ = walk_stmts(&file.stmts, Walk::ALL, |node, _| {
        if let AstNode::Expr(Expr::Call { func, .. }) = node
            && ident_name(func) == Some("box::export")
        {
            all_calls += 1;
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });
    if all_calls > 0 {
        return (names, complete && all_calls == direct_calls);
    }
    // box 1.2.3 falls back to legacy exports when tags name nothing.
    if roxygen.tagged && !(roxygen.complete && roxygen.names.is_empty()) {
        // An aliased or computed box::export() can extend a tagged module's
        // inventory during load. Only an effect-free body proves absence.
        return (roxygen.names, complete && roxygen.complete);
    }
    // Legacy modules export their own non-dot bindings. Imported names live
    // in an attachment environment and are not implicitly re-exported.
    // Dynamic writes or control flow make absence uncertain.
    complete &= !file.stmts.iter().any(|statement| {
        matches!(statement, Stmt::If { .. } | Stmt::For { .. } | Stmt::While { .. })
            || matches!(statement, Stmt::Expr(Expr::Call { func, .. }) if matches!(ident_name(func), Some("assign" | "delayedAssign" | "makeActiveBinding")))
    });
    assigned.retain(|name| !name.starts_with('.'));
    (assigned, complete)
}

/// Bind a selected or wildcard attachment. box attaches it in a parent of
/// the caller's environment, so an existing own binding stays visible; a
/// call skips an own non-function value and may reach the attachment.
/// Inside a function an existing binding may instead belong to an
/// enclosing frame, which the attachment shadows.
fn bind_attachment(scope: &mut Scope, name: &str, export: Option<&BoxExport>, top_level: bool) {
    if let Some(own) = scope.get(name)
        && !matches!(scope.box_objects.get(name), Some(BoxObject::Attached(_)))
    {
        let own_function = own.mode == Mode::Function;
        if !top_level {
            scope.insert(name.to_string(), RType::unknown());
        }
        if !top_level || !own_function {
            scope.set_box_object(name, BoxObject::OpaqueCall);
        }
        return;
    }
    let value = export.map_or_else(RType::unknown, |export| export.value.clone());
    let callable = export.and_then(|export| export.callable.clone());
    scope.insert(name.to_string(), value);
    scope.set_box_attachment(name, callable);
}

impl Scope {
    fn set_box_attachment(&mut self, name: &str, callable: Option<BoxCallable>) {
        // Other call stages see a package function through its alias.
        if let Some(BoxCallable::Package(target)) = &callable {
            self.set_function_alias(name, format!("{IMPORT_ALIAS}{target}"));
        }
        self.set_box_object(name, BoxObject::Attached(callable));
    }

    /// Install the provenance from `join_box_provenance` after a branch
    /// merge. Disagreeing paths leave a binding whose calls are opaque.
    pub(crate) fn apply_box_provenance(&mut self, joined: Vec<(String, Option<BoxObject>)>) {
        for (name, object) in joined {
            if self.box_objects.get(&name) == object.as_ref() {
                continue;
            }
            self.journal_binding(&name);
            if self
                .function_aliases
                .get(&name)
                .is_some_and(|alias| alias.starts_with(IMPORT_ALIAS))
            {
                self.function_aliases.remove(&name);
            }
            match object {
                Some(BoxObject::Attached(callable)) => self.set_box_attachment(&name, callable),
                Some(object) => {
                    self.box_objects.insert(name, object);
                }
                None => {
                    self.box_objects.remove(&name);
                }
            }
        }
    }
}

/// Branch joins compare binding types, which cannot tell two opaque module
/// objects or callables apart. For each name with a box object on some
/// path, return the object every reaching path agrees on, or `OpaqueCall`
/// when they differ. A `None` path is the pre-branch scope.
pub(crate) fn join_box_provenance(
    scope: &Scope,
    paths: &[Option<&BranchDelta>],
) -> Vec<(String, Option<BoxObject>)> {
    let mut names: HashSet<&str> = scope.box_objects.keys().map(String::as_str).collect();
    for delta in paths.iter().flatten() {
        names.extend(delta.box_object_names());
    }
    names
        .into_iter()
        .filter_map(|name| {
            let mut objects = paths.iter().map(|path| match path {
                Some(delta) => delta.box_object(scope, name),
                None => scope.box_objects.get(name),
            });
            let first = objects.next()?;
            let joined = if objects.all(|other| other == first) {
                first.cloned()
            } else {
                Some(BoxObject::OpaqueCall)
            };
            Some((name.to_string(), joined))
        })
        .collect()
}

/// `object$member` on a box object, with the member's decoded name.
pub(crate) fn box_member<'a>(
    expr: &'a Expr,
    scope: &Scope,
) -> Option<(&'a Expr, BoxObject, String)> {
    let Expr::Index {
        base,
        kind: IndexKind::Dollar,
        args,
        ..
    } = expr
    else {
        return None;
    };
    let object = match scope.box_objects.get(ident_name(base)?)? {
        object @ (BoxObject::Package(_)
        | BoxObject::Module(_)
        | BoxObject::Attached(Some(BoxCallable::Function(_)))) => object.clone(),
        // Other attachments keep their value's ordinary `$` inference.
        _ => return None,
    };
    let member = binding_name_token(args.first()?.name.as_deref()?)?;
    Some((base, object, member))
}

/// Read and parse an existing module file. The read is bounded as well as
/// the metadata precheck: the file can grow between those operations.
fn read_module(path: &Path, identity: &Path) -> Option<SourceFile> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_MODULE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_MODULE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let decoded = ry_workspace::decode_r_source(&bytes);
    let mut file = ry_core::RParser::new()
        .ok()?
        .parse(&path.to_string_lossy(), &decoded.text)
        .ok()?;
    decoded.attach_boundary_findings(&mut file);
    file.native_path = Some(identity.to_path_buf());
    Some(file)
}

impl Checker {
    fn box_caller(&self) -> &Path {
        self.native_path.as_deref().unwrap_or(Path::new(&self.path))
    }

    /// Literal NAMESPACE readers do not interpret exportPattern, S3
    /// registrations, or loader hooks, so package absence is never proof.
    fn box_package_inventory(&mut self, package: &str) -> Arc<BoxInventory> {
        let key = (self.box_caller().to_path_buf(), package.to_string());
        if let Some(inventory) = self.box_package_cache.get(&key) {
            return Arc::clone(inventory);
        }
        let mut exports = BTreeMap::new();
        if let Some(typeshed) = self.package_typeshed(package) {
            for name in typeshed.functions.keys() {
                exports.insert(name.clone(), RType::scalar(Mode::Function));
            }
            for (name, value) in &typeshed.datasets {
                exports.insert(name.clone(), infer::json_rtype_to_rtype(value));
            }
        }
        if let Some(installed) = ry_workspace::installed_exports_for_file(package, &key.0) {
            exports.retain(|name, _| installed.contains(name));
            for name in installed {
                exports.entry(name).or_insert_with(RType::unknown);
            }
        }
        let exports = exports
            .into_iter()
            .map(|(name, value)| {
                let callable = (value.mode == Mode::Function)
                    .then(|| BoxCallable::Package(format!("{package}::{name}")));
                (name, BoxExport { value, callable })
            })
            .collect();
        let inventory = Arc::new(BoxInventory {
            exports,
            complete: false,
        });
        self.box_package_cache.insert(key, Arc::clone(&inventory));
        inventory
    }

    fn box_module_inventory(&mut self, segments: &[String]) -> Option<Arc<BoxInventory>> {
        if self.box_depth >= MAX_MODULE_DEPTH {
            return None;
        }
        for candidate in module_candidates(self.box_caller(), segments)? {
            let identity = path_identity(&candidate)?;
            if let Some(inventory) = self.box_module_cache.get(&identity) {
                return Some(Arc::clone(inventory));
            }
            let mut file = match self.box_sources.get(&identity) {
                Some(Some(source)) => Arc::clone(source),
                // An open buffer outside this check: its disk copy may be stale.
                Some(None) => return None,
                // Only an absent candidate permits the next spelling. An
                // existing but unsupported preferred `.r` is the module box
                // selects; `.R` would describe a different module.
                None => match std::fs::symlink_metadata(&candidate) {
                    Ok(_) => Arc::new(read_module(&candidate, &identity)?),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => return None,
                },
            };
            // Source that R's parser rejects (#376, #474) cannot provide a
            // trustworthy inventory, whether read from disk or a buffer.
            if file.source.len() as u64 > MAX_MODULE_BYTES
                || file.leading_bom
                || !file.invalid_utf8.is_empty()
                || !file.parse_errors.is_empty()
                || !file.syntax_violations.is_empty()
            {
                return None;
            }
            // box loads a module from its canonical path, so its own
            // relative imports resolve beside the symlink target.
            if file.native_path.as_deref() != Some(identity.as_path()) {
                let mut canonical = SourceFile::clone(&file);
                canonical.native_path = Some(identity.clone());
                file = Arc::new(canonical);
            }
            let inventory = Arc::new(self.analyze_box_module(&file));
            self.box_module_cache
                .insert(identity, Arc::clone(&inventory));
            return Some(inventory);
        }
        None
    }

    fn analyze_box_module(&self, file: &SourceFile) -> BoxInventory {
        let (mut assigned, roxygen) = top_level_bindings(file);
        let load = module_load(file);
        // Legacy box modules export bindings made by executed assignment
        // expressions too (`a <- b <- 1L`, `dummy <- (foo <- 1L)`).
        // Roxygen/explicit inventories still use their declared names.
        assigned.extend(load.own_expression_names.iter().cloned());
        let stable_functions = stable_direct_function_bindings(file, &load);
        let (exported, complete) =
            declared_exports(file, assigned, roxygen, load.unmodeled_effects);
        let mut nested = Checker::new(&file.path);
        nested.box_depth = self.box_depth + 1;
        nested.box_sources = Arc::clone(&self.box_sources);
        nested.set_user_stubs(Arc::clone(&self.user_stubs));
        let (_, scope) = nested.check_with_scope(file);
        let mut inventory = BoxInventory {
            complete: complete && exported.iter().all(|name| scope.get(name).is_some()),
            ..BoxInventory::default()
        };
        for name in exported {
            let value = scope.get(&name);
            let written = load.counts.contains_key(&name);
            // An imported callable keeps its provenance only when nothing
            // at module load may replace it.
            let imported = !load.unmodeled_effects && !written;
            let alias = scope.function_alias(&name);
            let callable = match (scope.box_objects.get(&name), alias) {
                (Some(BoxObject::Attached(callable)), _) if imported => callable.clone(),
                (_, None)
                    if stable_functions.contains(&name)
                        && value.is_some_and(|value| value.mode == Mode::Function) =>
                {
                    // Refinement starts from formals, without the module's
                    // lexical imports. It can borrow a base signature for a
                    // replaced name, so retain formals with an unknown return.
                    nested.fn_table.fns.get(&name).map(|function| {
                        BoxCallable::Function(Arc::new(BoxFunction {
                            params: function.params.clone(),
                            return_type: if load.imports {
                                RType::unknown()
                            } else {
                                nested.return_slots.get(function.return_slot)
                            },
                        }))
                    })
                }
                _ => None,
            };
            let stale_callable = value.is_some_and(|value| value.mode == Mode::Function)
                && written
                && !stable_functions.contains(&name);
            let value = if load.unmodeled_effects
                || load.expression_names.contains(&name)
                || stale_callable
            {
                RType::unknown()
            } else {
                value.cloned().unwrap_or_else(RType::unknown)
            };
            inventory
                .exports
                .insert(name, BoxExport { value, callable });
        }
        inventory
    }

    fn infer_box_use(&mut self, args: &[Arg], scope: &mut Scope) -> RType {
        for argument in args {
            let Some(import) = parse_import(argument) else {
                // A computed argument is outside the static model. box
                // quotes it, so evaluating it here would invent RY010s.
                if let Some(alias) = &argument.name {
                    scope.insert(alias.clone(), RType::unknown());
                }
                continue;
            };
            let (inventory, object) = match import.target {
                Target::Package(package) => (
                    Some(self.box_package_inventory(&package)),
                    Some(BoxObject::Package(package)),
                ),
                Target::Module(segments) => {
                    let inventory = self.box_module_inventory(&segments);
                    let object = inventory.clone().map(BoxObject::Module);
                    (inventory, object)
                }
                Target::UnresolvedModule => (None, None),
            };
            if let Some(object_name) = import.object_name {
                // box module objects are environments, not lists. Their
                // exact export lookup lives in `BoxObject`; an opaque type
                // avoids inventing list schema or length facts elsewhere.
                scope.insert(object_name.clone(), RType::unknown());
                if let Some(object) = &object {
                    scope.set_box_object(object_name, object.clone());
                }
            }
            let top_level = self.enclosing_formals.is_empty();
            let selections = import.selection.unwrap_or_default();
            for selection in &selections {
                let export = inventory
                    .as_ref()
                    .and_then(|inventory| inventory.exports.get(&selection.original));
                if export.is_none()
                    && inventory
                        .as_ref()
                        .is_some_and(|inventory| inventory.complete)
                {
                    self.emit(
                        Severity::Warning,
                        selection.span,
                        "RY118",
                        format!("box module does not export `{}`", selection.original),
                    );
                }
                bind_attachment(scope, &selection.bound, export, top_level);
            }
            if import.wildcard {
                if let Some(inventory) = &inventory {
                    // Renaming an export removes its original spelling.
                    for (name, export) in &inventory.exports {
                        if !selections.iter().any(|selection| {
                            selection.original == *name && selection.bound != *name
                        }) {
                            bind_attachment(scope, name, Some(export), top_level);
                        }
                    }
                }
                scope.search_path_unknown |= !inventory.is_some_and(|inventory| inventory.complete);
            }
            scope.search_path_unknown |= import.selection_unknown;
        }
        RType::new(Mode::Null, Length::Zero)
    }

    /// Exact `$` lookup on a box object. A module inventory can prove a
    /// missing export; package metadata remains open-world.
    pub(crate) fn infer_box_member(
        &mut self,
        object: &BoxObject,
        member: &str,
        span: Span,
    ) -> RType {
        let inventory = match object {
            BoxObject::Package(package) => self.box_package_inventory(package),
            BoxObject::Module(inventory) => Arc::clone(inventory),
            BoxObject::Attached(_) | BoxObject::OpaqueCall => return RType::unknown(),
        };
        if let Some(export) = inventory.exports.get(member) {
            return export.value.clone();
        }
        if inventory.complete {
            self.emit(
                Severity::Warning,
                span,
                "RY118",
                format!("box module does not export `{member}`"),
            );
        }
        RType::unknown()
    }

    /// Calls through box objects, imported functions, and `box::` itself.
    /// `None` leaves the call to the ordinary resolution path.
    pub(crate) fn infer_box_call(
        &mut self,
        func: &Expr,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
        environment_known_before_call: bool,
    ) -> Option<RType> {
        if let Some((base, object, member)) = box_member(func, scope) {
            match object {
                BoxObject::Package(package) => {
                    self.infer(base, scope);
                    if self
                        .box_package_inventory(&package)
                        .exports
                        .contains_key(&member)
                    {
                        let callable = BoxCallable::Package(format!("{package}::{member}"));
                        return Some(self.infer_box_callable(
                            &member,
                            &callable,
                            args,
                            scope,
                            span,
                            environment_known_before_call,
                        ));
                    }
                    self.infer_args_for_diagnostics(args, scope);
                    return Some(RType::unknown());
                }
                BoxObject::Module(inventory) => {
                    // Infer the member itself for its RY118.
                    let member_type = self.infer(func, scope);
                    let callable = inventory
                        .exports
                        .get(&member)
                        .and_then(|export| export.callable.clone());
                    if let Some(callable) = callable {
                        return Some(self.infer_box_callable(
                            &member,
                            &callable,
                            args,
                            scope,
                            span,
                            environment_known_before_call,
                        ));
                    }
                    self.infer_args_for_diagnostics(args, scope);
                    return Some(
                        member_type
                            .fn_sig
                            .map(|signature| (*signature.return_type).clone())
                            .unwrap_or_else(RType::unknown),
                    );
                }
                BoxObject::Attached(_) | BoxObject::OpaqueCall => {}
            }
        }
        let name = binding_name(func)?;
        let opaque = match scope.box_objects.get(name).cloned() {
            Some(BoxObject::Attached(Some(callable))) => {
                return Some(self.infer_box_callable(
                    name,
                    &callable,
                    args,
                    scope,
                    span,
                    environment_known_before_call,
                ));
            }
            Some(BoxObject::OpaqueCall) => true,
            // An opaque attachment may be a function, so base or project
            // signatures of the same name do not describe the call.
            Some(BoxObject::Attached(None)) => scope
                .get(name)
                .is_some_and(|value| value.mode == Mode::Opaque),
            _ => false,
        };
        if opaque {
            self.infer_args_for_diagnostics(args, scope);
            return Some(RType::unknown());
        }
        match name {
            "box::use" => Some(self.infer_box_use(args, scope)),
            "box::export" => Some(RType::new(Mode::Null, Length::Zero)),
            _ => None,
        }
    }

    fn infer_box_callable(
        &mut self,
        name: &str,
        callable: &BoxCallable,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
        environment_known_before_call: bool,
    ) -> RType {
        match callable {
            BoxCallable::Function(function) => {
                self.infer_box_function_call(name, function, args, scope, span)
            }
            BoxCallable::Package(target) => {
                let callee = Expr::Ident {
                    name: target.clone(),
                    span,
                };
                self.infer_call_inner(&callee, args, scope, span, environment_known_before_call)
            }
        }
    }

    fn infer_box_function_call(
        &mut self,
        name: &str,
        function: &BoxFunction,
        args: &[Arg],
        scope: &mut Scope,
        span: Span,
    ) -> RType {
        let names: Vec<&str> = function
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect();
        let matches = infer::match_arguments(&names, args);
        if self.validate_user_call_arguments {
            self.check_box_call_arguments(name, &function.params, args, &matches, span);
        }
        for (index, argument) in args.iter().enumerate() {
            let parameter = matches.param_for_arg[index]
                .or(matches.dots)
                .and_then(|index| function.params.get(index));
            match parameter {
                Some(parameter) if parameter.quoting => {
                    let mut quoted = scope.independent_execution_scope();
                    self.infer_discarding(&argument.value, &mut quoted);
                }
                Some(parameter) if parameter.defused => {
                    let mut masked = scope.clone().with_unknown_data_mask();
                    self.infer(&argument.value, &mut masked);
                }
                _ => {
                    self.infer(&argument.value, scope);
                }
            }
        }
        function.return_type.clone()
    }
}
