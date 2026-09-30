//! Static, bounded interpretation of box's quoted import syntax. This never
//! loads a module or a package: only parsed project buffers, local module
//! source, installed NAMESPACE declarations, and typeshed are consulted.

use crate::*;
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

const MAX_MODULE_BYTES: u64 = 1_048_576;
const MAX_MODULE_DEPTH: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BoxObject {
    Package(String),
    Module(Arc<BoxInventory>),
    ModuleFunction(Arc<BoxFunction>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoxParam {
    pub(crate) name: String,
    pub(crate) required: bool,
    quoting: bool,
    defused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoxFunction {
    params: Vec<BoxParam>,
    return_type: RType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoxInventory {
    pub(crate) exports: BTreeMap<String, RType>,
    pub(crate) functions: BTreeMap<String, Arc<BoxFunction>>,
    pub(crate) package_functions: BTreeMap<String, String>,
    /// False if a dynamic export declaration or unmodeled source effect may
    /// add names; absence then cannot support RY118.
    pub(crate) complete: bool,
}

#[derive(Debug)]
enum Target {
    Package(String),
    Module(Vec<String>),
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

fn static_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => {
            Some(infer::semantic_argument_name(name))
        }
        _ => None,
    }
}

fn path_segments(expr: &Expr, segments: &mut Vec<String>) -> bool {
    match expr {
        Expr::Ident { name, .. } => {
            segments.push(infer::semantic_argument_name(name).to_string());
            true
        }
        Expr::BinOp {
            op: BinOpKind::Div,
            lhs,
            rhs,
            ..
        } => path_segments(lhs, segments) && path_segments(rhs, segments),
        _ => false,
    }
}

fn parse_import(argument: &Arg) -> Option<Import> {
    let mut selections = None;
    let mut wildcard = false;
    let mut selection_unknown = false;
    // The parser puts `[...]` on the final path component, not on the
    // entire `/` expression: `./a/b[x]` is `./a/(b[x])` in the AST.
    fn strip_selection<'a>(
        expr: &'a Expr,
        selections: &mut Option<Vec<Selection>>,
        wildcard: &mut bool,
        unknown: &mut bool,
    ) -> Option<&'a Expr> {
        match expr {
            Expr::Index {
                base,
                kind: IndexKind::Single,
                args,
                ..
            } => {
                if selections.is_some() {
                    return None;
                }
                let mut picked = Vec::new();
                for arg in args {
                    match static_name(&arg.value) {
                        Some("...") if arg.name.is_none() => *wildcard = true,
                        Some(original) if original != "..." => picked.push(Selection {
                            original: original.to_string(),
                            bound: arg.name.clone().unwrap_or_else(|| original.to_string()),
                            span: arg.span,
                        }),
                        _ => *unknown = true,
                    }
                }
                *selections = Some(picked);
                Some(base)
            }
            Expr::BinOp {
                op: BinOpKind::Div,
                rhs,
                ..
            } if matches!(rhs.as_ref(), Expr::Index { .. }) => {
                strip_selection(rhs, selections, wildcard, unknown)?;
                Some(expr)
            }
            _ => Some(expr),
        }
    }
    let core = strip_selection(
        &argument.value,
        &mut selections,
        &mut wildcard,
        &mut selection_unknown,
    )?;
    let mut segments = Vec::new();
    if !path_segments(core, &mut segments) {
        // In a `/` chain with a selected final component, strip that final
        // index while retaining every preceding literal path component.
        fn selected_segments(expr: &Expr, segments: &mut Vec<String>) -> bool {
            match expr {
                Expr::BinOp {
                    op: BinOpKind::Div,
                    lhs,
                    rhs,
                    ..
                } => path_segments(lhs, segments) && selected_segments(rhs, segments),
                Expr::Index { base, .. } => path_segments(base, segments),
                _ => path_segments(expr, segments),
            }
        }
        segments.clear();
        if !selected_segments(core, &mut segments) {
            return None;
        }
    }
    let basename = segments.last()?.clone();
    let target = match segments.as_slice() {
        [package] if package != "." && package != ".." => Target::Package(package.clone()),
        [first, ..] if first == "." || first == ".." => Target::Module(segments),
        _ => return None,
    };
    // A selective import binds only the selected names unless the import
    // explicitly names the module/package object.
    let object_name = if selections.is_none() || argument.name.is_some() {
        Some(argument.name.clone().unwrap_or(basename))
    } else {
        None
    };
    Some(Import {
        target,
        object_name,
        selection: selections,
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

/// Calls made while loading a legacy module may create caller-frame
/// bindings, including through qualified writers and sourced files. We do
/// not execute them, so a missing syntactic assignment is not proof that a
/// name is absent. Function bodies are inert until called; box's quoted
/// import declaration itself does not write an own-module binding.
fn has_unmodeled_load_effects(file: &SourceFile) -> bool {
    let mut uncertain = false;
    let _ = walk_stmts(
        &file.stmts,
        Walk {
            fn_bodies: false,
            ..Walk::ALL
        },
        |node, _| {
            match node {
                AstNode::Expr(Expr::Function { .. }) => {
                    return ControlFlow::<(), Descend>::Continue(Descend::Skip);
                }
                AstNode::Expr(Expr::Call { func, args, .. })
                    if ident_name(func) == Some("box::use") =>
                {
                    uncertain |= args.iter().any(|argument| parse_import(argument).is_none());
                    return ControlFlow::<(), Descend>::Continue(Descend::Skip);
                }
                AstNode::Expr(Expr::Call { func, args, .. })
                    if ident_name(func) == Some("box::export")
                        && args
                            .iter()
                            .all(|argument| static_name(&argument.value).is_some()) =>
                {
                    return ControlFlow::<(), Descend>::Continue(Descend::Skip);
                }
                AstNode::Expr(Expr::Call { .. }) => uncertain = true,
                // Executed braced and conditional expressions can write
                // module bindings without producing a top-level Stmt::Assign.
                AstNode::Expr(Expr::Block { .. } | Expr::If { .. }) => uncertain = true,
                _ => {}
            }
            ControlFlow::<(), Descend>::Continue(Descend::Into)
        },
    );
    uncertain
}

/// The collector indexes literal functions by spelling. A later wrapper
/// assignment can leave the final value callable while the indexed literal
/// is no longer its definition. Retain the collected signature only for a
/// single direct literal binding with no other module-load assignment to the
/// same name. Control-flow bindings are counted; nested function bodies have
/// their own lexical scope and are excluded.
fn stable_direct_function_bindings(
    file: &SourceFile,
    unmodeled_load_effects: bool,
    writes: &ModuleWrites,
) -> HashSet<String> {
    if unmodeled_load_effects {
        return HashSet::new();
    }
    let mut direct_literals = HashSet::new();
    for statement in &file.stmts {
        if let Stmt::Assign {
            target,
            value: Expr::Function { .. },
            ..
        } = statement
            && let Some(name) = binding_name(target)
        {
            direct_literals.insert(infer::semantic_argument_name(name).to_string());
        }
    }
    direct_literals.retain(|name| writes.counts.get(name) == Some(&1));
    direct_literals
}

#[derive(Default)]
struct ModuleWrites {
    counts: HashMap<String, usize>,
    names: HashSet<String>,
    expression_names: HashSet<String>,
}

fn module_writes(file: &SourceFile) -> ModuleWrites {
    let mut writes = ModuleWrites::default();
    let _ = walk_stmts(
        &file.stmts,
        Walk {
            fn_bodies: false,
            ..Walk::ALL
        },
        |node, _| {
            let binding = match node {
                AstNode::Stmt(Stmt::Assign { target, .. }) => binding_name(target),
                AstNode::Expr(Expr::BinOp {
                    op: BinOpKind::Assign | BinOpKind::SuperAssign | BinOpKind::PipeAssign,
                    lhs,
                    ..
                }) => {
                    let name = binding_name(lhs);
                    if let Some(name) = name {
                        writes
                            .expression_names
                            .insert(infer::semantic_argument_name(name).to_string());
                    }
                    name
                }
                _ => None,
            };
            if let Some(name) = binding {
                let name = infer::semantic_argument_name(name).to_string();
                *writes.counts.entry(name.clone()).or_default() += 1;
                writes.names.insert(name);
            }
            ControlFlow::<(), Descend>::Continue(Descend::Into)
        },
    );
    writes
}

fn top_level_bindings(file: &SourceFile) -> (HashSet<String>, HashSet<String>) {
    let mut assigned = HashSet::new();
    let mut roxygen = HashSet::new();
    let lines: Vec<&str> = file.source.lines().collect();
    for statement in &file.stmts {
        if let Stmt::Expr(Expr::Call { func, args, .. }) = statement
            && ident_name(func) == Some("box::use")
        {
            for argument in args {
                if let Some(import) = parse_import(argument)
                    && let Some(name) = import.object_name
                {
                    // box stores module/package objects in its own module
                    // environment. Selectively attached names without an
                    // object alias live in the parent imports environment.
                    assigned.insert(name);
                }
            }
        }
        let Stmt::Assign { target, span, .. } = statement else {
            continue;
        };
        let Some(name) = binding_name(target) else {
            continue;
        };
        let name = infer::semantic_argument_name(name).to_string();
        assigned.insert(name.clone());
        let mut before = span.line;
        while before > 0 {
            before -= 1;
            let text = lines.get(before).copied().unwrap_or("").trim_start();
            if !text.starts_with("#'") {
                break;
            }
            if text[2..].trim_start().starts_with("@export") {
                roxygen.insert(name.clone());
            }
        }
    }
    (assigned, roxygen)
}

fn declared_exports(
    file: &SourceFile,
    assigned: &HashSet<String>,
    roxygen: HashSet<String>,
    unmodeled_load_effects: bool,
) -> (HashSet<String>, bool) {
    let mut explicit = false;
    let mut complete = true;
    let mut names = HashSet::new();
    let mut direct = HashSet::new();
    for statement in &file.stmts {
        if let Stmt::Expr(Expr::Call { func, args, span }) = statement
            && ident_name(func) == Some("box::export")
        {
            explicit = true;
            direct.insert(span.start);
            for argument in args {
                if let Some(name) = static_name(&argument.value) {
                    names.insert(name.to_string());
                } else {
                    complete = false;
                }
            }
        }
    }
    // A braced, conditional, or otherwise nested export call may add names
    // at runtime. It also prevents legacy fallback, even when it declares
    // no statically readable name.
    let _ = walk_stmts(&file.stmts, Walk::ALL, |node, _| {
        if let AstNode::Expr(Expr::Call { func, span, .. }) = node
            && ident_name(func) == Some("box::export")
            && !direct.contains(&span.start)
        {
            explicit = true;
            complete = false;
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });
    if explicit {
        return (names, complete && !unmodeled_load_effects);
    }
    if !roxygen.is_empty() {
        return (roxygen, complete);
    }
    // Legacy modules export their own non-dot bindings. Imported names live
    // in an attachment environment and are not implicitly re-exported.
    // Dynamic writes or control flow make absence uncertain.
    complete &= !unmodeled_load_effects && !file.stmts.iter().any(|statement| {
        matches!(statement, Stmt::If { .. } | Stmt::For { .. } | Stmt::While { .. })
            || matches!(statement, Stmt::Expr(Expr::Call { func, .. }) if matches!(ident_name(func), Some("assign" | "delayedAssign" | "makeActiveBinding")))
    });
    (
        assigned
            .iter()
            .filter(|name| !name.starts_with('.'))
            .cloned()
            .collect(),
        complete,
    )
}

impl Checker {
    pub(crate) fn box_package_inventory(&self, package: &str) -> BoxInventory {
        let mut exports = BTreeMap::new();
        if let Some(typeshed) = self.package_typeshed(package) {
            for name in typeshed.functions.keys() {
                exports.insert(name.clone(), RType::scalar(Mode::Function));
            }
            for (name, value) in &typeshed.datasets {
                exports.insert(name.clone(), infer::json_rtype_to_rtype(value));
            }
        }
        if let Some(installed) = ry_workspace::installed_exports_for_file(
            package,
            self.native_path.as_deref().unwrap_or(Path::new(&self.path)),
        ) {
            exports.retain(|name, _| installed.contains(name));
            for name in installed {
                exports.entry(name).or_insert_with(RType::unknown);
            }
        }
        // Literal NAMESPACE readers do not interpret exportPattern, S3
        // registrations, or loader hooks. Package absence is never proof.
        BoxInventory {
            exports,
            functions: BTreeMap::new(),
            package_functions: BTreeMap::new(),
            complete: false,
        }
    }

    fn box_module_inventory(&mut self, segments: &[String]) -> Option<Arc<BoxInventory>> {
        if self.box_depth >= MAX_MODULE_DEPTH {
            return None;
        }
        let caller = self.native_path.as_deref().unwrap_or(Path::new(&self.path));
        for candidate in module_candidates(caller, segments)? {
            let identity = path_identity(&candidate)?;
            if let Some(inventory) = self.box_module_cache.get(&identity) {
                return inventory.clone();
            }
            let file = if let Some(source) = self.box_sources.get(&identity) {
                if source.source.len() as u64 > MAX_MODULE_BYTES {
                    return None;
                }
                Arc::clone(source)
            } else {
                // Only an absent candidate permits the next spelling.
                // Existing but unreadable/unsupported preferred `.r` is the
                // module box selects; consulting `.R` would invent facts
                // about a different module.
                match std::fs::symlink_metadata(&candidate) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => return None,
                }
                let metadata = std::fs::metadata(&candidate).ok()?;
                if !metadata.is_file() || metadata.len() > MAX_MODULE_BYTES {
                    return None;
                }
                // Bound the actual read as well as the metadata precheck:
                // the file can grow or be replaced between those operations.
                let mut bytes = Vec::new();
                std::fs::File::open(&candidate)
                    .ok()?
                    .take(MAX_MODULE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                if bytes.len() as u64 > MAX_MODULE_BYTES || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
                    return None;
                }
                // Invalid UTF-8 is an R parser boundary finding in the
                // frontends. An imported module with such bytes cannot
                // provide a trustworthy export inventory.
                let source = String::from_utf8(bytes).ok()?;
                let mut parser = ry_core::RParser::new().ok()?;
                let mut parsed = parser.parse(&candidate.to_string_lossy(), &source).ok()?;
                parsed.native_path = Some(candidate.clone());
                Arc::new(parsed)
            };
            if !file.parse_errors.is_empty() || !file.syntax_violations.is_empty() {
                return None;
            }
            let (assigned, roxygen) = top_level_bindings(&file);
            let unmodeled_load_effects = has_unmodeled_load_effects(&file);
            let writes = module_writes(&file);
            let stable_functions =
                stable_direct_function_bindings(&file, unmodeled_load_effects, &writes);
            let (exported, complete) =
                declared_exports(&file, &assigned, roxygen, unmodeled_load_effects);
            let mut nested = Checker::new(&file.path);
            nested.box_depth = self.box_depth + 1;
            nested.box_sources = Arc::clone(&self.box_sources);
            nested.set_user_stubs(Arc::clone(&self.user_stubs));
            let (_, scope) = nested.check_with_scope(&file);
            let complete = complete && exported.iter().all(|name| scope.get(name).is_some());
            let package_functions = exported
                .iter()
                .filter_map(|name| {
                    if unmodeled_load_effects || writes.names.contains(name) {
                        return None;
                    }
                    scope
                        .function_alias(name)?
                        .strip_prefix("__ry_box_import::")
                        .map(|target| (name.clone(), target.to_string()))
                })
                .collect();
            let functions = exported
                .iter()
                .filter_map(|name| {
                    if !unmodeled_load_effects
                        && !writes.names.contains(name)
                        && let Some(BoxObject::ModuleFunction(function)) =
                            scope.box_objects.get(name)
                    {
                        return Some((name.clone(), Arc::clone(function)));
                    }
                    if scope.function_alias(name).is_some() {
                        return None;
                    }
                    if !stable_functions.contains(name) {
                        return None;
                    }
                    let function = nested.fn_table.fns.get(name)?;
                    if scope.get(name)?.mode != Mode::Function {
                        return None;
                    }
                    let return_type = nested.return_slots.0.get(function.return_slot)?.clone();
                    let params = function
                        .params
                        .iter()
                        .map(|parameter| BoxParam {
                            name: parameter.name.clone(),
                            required: parameter.required,
                            quoting: parameter.quoting,
                            defused: parameter.defused,
                        })
                        .collect();
                    Some((
                        name.clone(),
                        Arc::new(BoxFunction {
                            params,
                            return_type,
                        }),
                    ))
                })
                .collect();
            let exports = exported
                .into_iter()
                .map(|name| {
                    let stale_callable = scope.get(&name).is_some_and(|value| {
                        value.mode == Mode::Function
                            && writes.names.contains(&name)
                            && !stable_functions.contains(&name)
                    });
                    let value = if unmodeled_load_effects
                        || writes.expression_names.contains(&name)
                        || stale_callable
                    {
                        RType::unknown()
                    } else {
                        scope.get(&name).cloned().unwrap_or_else(RType::unknown)
                    };
                    (name, value)
                })
                .collect();
            let inventory = Arc::new(BoxInventory {
                exports,
                functions,
                package_functions,
                complete,
            });
            self.box_module_cache
                .insert(identity, Some(Arc::clone(&inventory)));
            return Some(inventory);
        }
        None
    }

    pub(crate) fn infer_box_use(&mut self, args: &[Arg], scope: &mut Scope) -> RType {
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
                Target::Package(package) => {
                    let inventory = Arc::new(self.box_package_inventory(&package));
                    (Some(inventory), Some(BoxObject::Package(package)))
                }
                Target::Module(segments) => {
                    let inventory = self.box_module_inventory(&segments);
                    let object = inventory
                        .as_ref()
                        .map(|inventory| BoxObject::Module(Arc::clone(inventory)));
                    (inventory, object)
                }
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
            if let Some(selections) = import.selection {
                let renamed: HashSet<String> = selections
                    .iter()
                    .filter(|selection| selection.original != selection.bound)
                    .map(|selection| selection.original.clone())
                    .collect();
                for selection in selections {
                    let missing = inventory.as_ref().is_some_and(|inventory| {
                        inventory.complete && !inventory.exports.contains_key(&selection.original)
                    });
                    if missing {
                        self.emit(
                            Severity::Warning,
                            selection.span,
                            "RY118",
                            format!("box module does not export `{}`", selection.original),
                        );
                    }
                    let value = if missing {
                        RType::unknown()
                    } else {
                        inventory
                            .as_ref()
                            .and_then(|inventory| inventory.exports.get(&selection.original))
                            .cloned()
                            .unwrap_or_else(RType::unknown)
                    };
                    scope.insert(selection.bound.clone(), value.clone());
                    if let Some(BoxObject::Module(module)) = &object
                        && let Some(function) = module.functions.get(&selection.original)
                    {
                        scope.set_box_object(
                            selection.bound.clone(),
                            BoxObject::ModuleFunction(Arc::clone(function)),
                        );
                    }
                    if let Some(BoxObject::Module(module)) = &object
                        && let Some(target) = module.package_functions.get(&selection.original)
                    {
                        scope.set_function_alias(
                            selection.bound.clone(),
                            format!("__ry_box_import::{target}"),
                        );
                    }
                    if matches!(value.mode, Mode::Function)
                        && let Some(BoxObject::Package(package)) = &object
                    {
                        scope.set_function_alias(
                            selection.bound,
                            format!("__ry_box_import::{package}::{}", selection.original),
                        );
                    }
                }
                if import.wildcard {
                    if let Some(inventory) = &inventory {
                        for (name, value) in &inventory.exports {
                            if renamed.contains(name.as_str()) {
                                continue;
                            }
                            scope.insert(name.clone(), value.clone());
                            if let Some(BoxObject::Module(module)) = &object
                                && let Some(function) = module.functions.get(name)
                            {
                                scope.set_box_object(
                                    name.clone(),
                                    BoxObject::ModuleFunction(Arc::clone(function)),
                                );
                            }
                            if let Some(BoxObject::Module(module)) = &object
                                && let Some(target) = module.package_functions.get(name)
                            {
                                scope.set_function_alias(
                                    name.clone(),
                                    format!("__ry_box_import::{target}"),
                                );
                            }
                            if matches!(value.mode, Mode::Function)
                                && let Some(BoxObject::Package(package)) = &object
                            {
                                scope.set_function_alias(
                                    name.clone(),
                                    format!("__ry_box_import::{package}::{name}"),
                                );
                            }
                        }
                        if !inventory.complete {
                            scope.search_path_unknown = true;
                        }
                    } else {
                        scope.search_path_unknown = true;
                    }
                }
                if import.selection_unknown {
                    scope.search_path_unknown = true;
                }
            }
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
        match object {
            BoxObject::Package(package) => {
                let inventory = self.box_package_inventory(package);
                inventory
                    .exports
                    .get(member)
                    .cloned()
                    .unwrap_or_else(RType::unknown)
            }
            BoxObject::Module(inventory) => {
                if let Some(value) = inventory.exports.get(member) {
                    value.clone()
                } else {
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
            }
            BoxObject::ModuleFunction(_) => RType::unknown(),
        }
    }

    pub(crate) fn infer_box_function_call(
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
