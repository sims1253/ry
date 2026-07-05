//! Document- and workspace-symbol collection.

use ry_checker::Scope;
use ry_core::{Expr, Span, Stmt};
use tower_lsp::lsp_types::{
    DocumentSymbol, Location, Position, Range, SymbolInformation, SymbolKind, Url,
};

use crate::util::span_to_range;

/// Recursively flatten a tree of `DocumentSymbol`s (with their
/// children) into a flat list of `SymbolInformation`s, attaching the
/// given URI to each symbol's `Location`. Workspace symbols is a flat
/// list per the LSP spec, so the hierarchical structure produced by
/// `collect_symbols` (which nests function-body bindings as children)
/// must be flattened before it can be returned to the editor.
///
/// Each `SymbolInformation` carries:
///   * the symbol's `name`, `kind`, `tags`, and `deprecated` flag
///     (copied straight from the source `DocumentSymbol`);
///   * a `Location` whose `uri` is the file the symbol lives in and
///     whose `range` is the symbol's `selection_range` (the
///     identifier span, which is what editors jump to when the user
///     picks a workspace symbol);
///   * a `container_name` set to the enclosing symbol's name (or
///     `None` for top-level symbols), so the editor can render the
///     breadcrumb "file > function > variable" in the picker.
pub(super) fn flatten_symbols_to_symbol_info(
    symbols: Vec<DocumentSymbol>,
    uri: &Url,
) -> Vec<SymbolInformation> {
    let mut out = Vec::new();
    for sym in symbols {
        flatten_one_symbol(sym, uri, None, &mut out);
    }
    out
}

/// Recurse into a single `DocumentSymbol`, pushing its own
/// `SymbolInformation` to `out` and then walking each child with the
/// current symbol's name as the `container_name`. The recursion
/// preserves the source order of children, matching how
/// `collect_symbols` emits them.
#[allow(deprecated)]
fn flatten_one_symbol(
    sym: DocumentSymbol,
    uri: &Url,
    container_name: Option<&str>,
    out: &mut Vec<SymbolInformation>,
) {
    let name = sym.name.clone();
    let info = SymbolInformation {
        name: sym.name,
        kind: sym.kind,
        tags: sym.tags,
        deprecated: sym.deprecated,
        location: Location {
            uri: uri.clone(),
            // Use `selection_range` (the identifier span) rather than
            // the full `range` so the editor lands the cursor on the
            // symbol's name, not somewhere inside its body.
            range: sym.selection_range,
        },
        container_name: container_name.map(|s| s.to_string()),
    };
    out.push(info);
    if let Some(children) = sym.children {
        for child in children {
            flatten_one_symbol(child, uri, Some(&name), out);
        }
    }
}

/// Collect `DocumentSymbol`s for an outline view of the file. Walks
/// the given statements, emitting one symbol per binding
/// (`x <- ...`) and named function definition. Control-flow bodies
/// (`if` / `for` / `while`) are flattened into the current level so
/// that R's block-level bindings show up in the outline the way R
/// users mentally scope them. Function bodies are NOT flattened:
/// their local definitions become `children` of the enclosing
/// function symbol, producing a hierarchical outline.
///
/// `scope` carries inferred types for the top level only. Pass
/// `None` for nested scopes so locals fall back to the generic
/// "function" / "variable" detail strings.
pub(super) fn collect_symbols(
    stmts: &[Stmt],
    text: &str,
    scope: Option<&Scope>,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for stmt in stmts {
        collect_from_stmt(stmt, text, scope, &mut symbols);
    }
    symbols
}

/// Walk a single statement, appending any symbols it contributes to
/// `out`. Assignments and named function definitions become symbols
/// directly; `if` / `for` / `while` blocks are traversed so their
/// inner bindings appear at the current outline level.
fn collect_from_stmt(
    stmt: &Stmt,
    text: &str,
    scope: Option<&Scope>,
    out: &mut Vec<DocumentSymbol>,
) {
    match stmt {
        Stmt::Assign { .. } | Stmt::FunctionDef { .. } => {
            if let Some(sym) = stmt_to_symbol(stmt, text, scope) {
                out.push(sym);
            }
        }
        Stmt::If { then, else_, .. } => {
            for s in then {
                collect_from_stmt(s, text, scope, out);
            }
            if let Some(else_block) = else_ {
                for s in else_block {
                    collect_from_stmt(s, text, scope, out);
                }
            }
        }
        Stmt::For { body, .. } | Stmt::While { body, .. } => {
            for s in body {
                collect_from_stmt(s, text, scope, out);
            }
        }
        // Bare expressions, returns, and other statement forms do not
        // introduce named bindings, so they contribute no symbols.
        Stmt::Return { .. } | Stmt::Expr(_) => {}
    }
}

/// Build a `DocumentSymbol` for a binding-producing statement, or
/// return `None` if the statement does not yield an outline-worthy
/// symbol (e.g. an assignment to an index like `x[1] <- 2`).
fn stmt_to_symbol(stmt: &Stmt, text: &str, scope: Option<&Scope>) -> Option<DocumentSymbol> {
    match stmt {
        Stmt::Assign {
            target,
            value,
            span,
            ..
        } => {
            // Only bare-identifier targets (`x <- ...`) become symbols.
            // Complex targets (`df$col <- 1`, `x[1] <- 2`) are skipped:
            // they don't introduce a new name in the outline.
            let Expr::Ident {
                name,
                span: ident_span,
            } = target
            else {
                return None;
            };
            let is_function = matches!(value, Expr::Function { .. });
            let kind = if is_function {
                SymbolKind::FUNCTION
            } else {
                SymbolKind::VARIABLE
            };
            let detail = compute_detail(name, is_function, scope);
            let selection_range = ident_to_range(*ident_span, name);
            let range = span_to_range(text, *span).unwrap_or(selection_range);
            // For function-valued assignments, the body's local
            // definitions become children of this symbol so the
            // outline shows the function's internal structure.
            let children = function_body_symbols(value, text);
            Some(make_document_symbol(
                name.clone(),
                detail,
                kind,
                range,
                selection_range,
                children,
            ))
        }
        Stmt::FunctionDef {
            name: Some(n),
            span,
            params,
            body,
            ..
        } => {
            // `Stmt::FunctionDef` with a name is currently never
            // emitted by the parser (named functions come through as
            // `Assign` + `Expr::Function`), but we handle it for
            // completeness / future grammar changes. Children include
            // the parameters plus any nested definitions in the body.
            let detail = compute_detail(n, true, scope);
            let selection_range = span_start_range(*span, n);
            let range = span_to_range(text, *span).unwrap_or(selection_range);
            let mut children: Vec<DocumentSymbol> =
                params.iter().filter_map(param_to_symbol).collect();
            children.extend(collect_symbols(body, text, None));
            let children = if children.is_empty() {
                None
            } else {
                Some(children)
            };
            Some(make_document_symbol(
                n.clone(),
                detail,
                SymbolKind::FUNCTION,
                range,
                selection_range,
                children,
            ))
        }
        // Anonymous function defs (`name: None`) and any other shape
        // don't carry a name to show in the outline.
        _ => None,
    }
}

/// Collect child symbols for a function-valued expression. Returns
/// `None` when the expression is not a function literal or when the
/// body has no bindings, so that non-function symbols stay flat.
fn function_body_symbols(value: &Expr, text: &str) -> Option<Vec<DocumentSymbol>> {
    if let Expr::Function { params, body, .. } = value {
        let mut children: Vec<DocumentSymbol> = params.iter().filter_map(param_to_symbol).collect();
        children.extend(collect_symbols(body, text, None));
        if children.is_empty() {
            None
        } else {
            Some(children)
        }
    } else {
        None
    }
}

/// Build a `DocumentSymbol` for a function parameter. Parameters use
/// `SymbolKind::VARIABLE` (LSP has no dedicated parameter kind) and
/// their range covers exactly the parameter name.
fn param_to_symbol(param: &ry_core::Param) -> Option<DocumentSymbol> {
    let range = ident_to_range(param.span, &param.name);
    Some(make_document_symbol(
        param.name.clone(),
        Some("parameter".to_string()),
        SymbolKind::VARIABLE,
        range,
        range,
        None,
    ))
}

/// Compute the `detail` string for a symbol. When we have a checked
/// scope and the name resolves, we surface the inferred type (e.g.
/// `integer<len=1>`); otherwise we fall back to the coarse
/// "function" / "variable" label so the outline is never blank.
fn compute_detail(name: &str, is_function: bool, scope: Option<&Scope>) -> Option<String> {
    if let Some(s) = scope {
        if let Some(t) = s.get(name) {
            return Some(format!("{}", t));
        }
    }
    Some(if is_function { "function" } else { "variable" }.to_string())
}

/// Build an LSP `Range` covering exactly an identifier, using the
/// identifier's `Span` for the start position and the name's length
/// for the end column. This matches the convention used by
/// `span_to_location` for go-to-definition.
fn ident_to_range(span: Span, name: &str) -> Range {
    let start = Position {
        line: span.line as u32,
        character: span.col as u32,
    };
    let end = Position {
        line: span.line as u32,
        character: span.col as u32 + name.len() as u32,
    };
    Range { start, end }
}

/// Build a `Range` anchored at a span's start position, spanning
/// `name.len()` characters. Used for `Stmt::FunctionDef` where we
/// only have the enclosing statement span (no dedicated name span).
fn span_start_range(span: Span, name: &str) -> Range {
    ident_to_range(span, name)
}

/// Construct a `DocumentSymbol` with all fields filled. The
/// `deprecated` field is marked `#[deprecated]` in `lsp-types`, so we
/// allow the warning here rather than spread `#[allow(deprecated)]`
/// across every call site.
#[allow(deprecated)]
fn make_document_symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}
