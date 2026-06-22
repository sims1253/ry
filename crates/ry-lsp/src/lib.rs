//! ry language server. Publishes diagnostics for R files.
//!
//! This is a v1 LSP server built on top of `tower-lsp`. It supports:
//!   * `initialize` / `initialized` handshake
//!   * `textDocument/didOpen` (publishes diagnostics)
//!   * `textDocument/didChange` (incremental edits re-check and republish)
//!   * `textDocument/didClose` (clears diagnostics)
//!   * Document diagnostics via `textDocument/publishDiagnostics`
//!   * `textDocument/hover` (type at cursor)
//!   * `textDocument/definition` (go-to-definition for variables/functions)
//!   * `textDocument/documentSymbol` (outline view of the file's bindings)
//!   * Graceful shutdown via `shutdown` / `exit`
//!
//! Out of scope for v1: code actions, formatting, completion, and
//! workspace configuration requests. We DO read `ry.toml` for rule
//! severities in future revisions, but v1 ignores configuration
//! change notifications.
//!
//! To test manually:
//!   1. `echo 'Content-Length: 77\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}' | cargo run -q --bin ry -- server`
//!   2. A JSON-RPC response with the server's capabilities should come
//!      back on stdout. All tracing/logging goes to stderr so the
//!      stdout JSON-RPC stream is never corrupted.
//!
//! CRITICAL INVARIANT: the LSP protocol uses stdout for JSON-RPC framing.
//! Any tracing or log output that lands on stdout will corrupt the stream
//! and crash the client. All `tracing` output is routed to stderr via
//! the CLI's `tracing_subscriber` initialization before `run()` is called.

use ry_checker::{Diagnostic as RyDiagnostic, Project, Scope, Severity};
use ry_core::{Expr, RParser, SourceFile, Span, Stmt};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::Diagnostic as LspDiagnostic;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    /// Open documents: path -> current source text. Keeping every open
    /// document's text lets us rebuild a multi-file `Project` on each
    /// change so cross-file resolution (function defined in `a.R`
    /// visible from `b.R` when both are open in the editor) works.
    docs: HashMap<String, String>,
    /// Workspace root, set at `initialize`. Used only for diagnostics
    /// today; future revisions may use it to discover `ry.toml`.
    #[allow(dead_code)]
    root: Option<PathBuf>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let mut state = self.state.lock().await;
        state.root = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok());
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    // v1: send the whole document on each change. This
                    // avoids the complexity of incremental range sync
                    // and matches the common "re-check on save" mode of
                    // most R users. The client sends the full text in
                    // `changes[0]`.
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                // Enable `textDocument/definition` so the client can
                // request go-to-definition (Ctrl+click / "Go to
                // Definition"). The handler is `goto_definition` below.
                definition_provider: Some(OneOf::Left(true)),
                // Enable `textDocument/documentSymbol` so the client can
                // render an outline of the file's structure (functions,
                // variables) in the sidebar. The handler is
                // `document_symbol` below.
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "ry".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        tracing::info!("ry LSP initialized");
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let text = params.text_document.text.clone();
        self.update_doc(path, text).await;
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        // TextDocumentSyncKind::FULL means the whole new text arrives in
        // `changes[0]`. v1 does not implement incremental sync, so we
        // ignore any further entries in `content_changes`.
        if let Some(change) = params.content_changes.into_iter().next() {
            self.update_doc(path, change.text).await;
            self.publish_diagnostics(uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        {
            let mut state = self.state.lock().await;
            state.docs.remove(&path);
        }
        // Clear diagnostics for the closed document so stale squiggles
        // don't linger after the user closes the file.
        self.client
            .publish_diagnostics(uri, Vec::new(), None)
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let position = params.text_document_position_params.position;

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Find the identifier at the hover position. We look for a
        // word-like character sequence around the cursor.
        let identifier = find_identifier_at_position(&text, position.line as usize, position.character as usize);
        let Some(identifier) = identifier else {
            return Ok(None);
        };

        // Parse and check the file to get the scope.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let file = match parser.parse(&path, &text) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };
        let mut checker = ry_checker::Checker::new(&path);
        let (_, scope) = checker.check_with_scope(&file);

        // Look up the identifier in the scope.
        if let Some(t) = scope.get(&identifier) {
            let type_str = format!("{}", t);
            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("```r\n{}: {}\n```", identifier, type_str),
                }),
                range: None,
            }));
        }

        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let position = params.text_document_position_params.position;

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Reuse the same word-finding helper as `hover` to extract the
        // identifier under the cursor. Returns `None` (no definition)
        // for operators, numbers, and keywords.
        let identifier =
            find_identifier_at_position(&text, position.line as usize, position.character as usize);
        let Some(identifier) = identifier else {
            return Ok(None);
        };

        // Parse the current document. We do not need the checker's
        // scope here: definitions live in the AST, not the type
        // environment.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let file = match parser.parse(&path, &text) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        let locations = find_definition_locations(&file, &identifier, &uri);
        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(GotoDefinitionResponse::Array(locations)))
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Parse the document. On any parse failure we return `None`
        // (no symbols) rather than erroring, so the editor's outline
        // panel simply shows empty instead of a broken state.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let file = match parser.parse(&path, &text) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        // Run the checker so we can attach inferred types to the
        // `detail` field of each top-level symbol. Symbols nested
        // inside function bodies fall back to "function" / "variable"
        // since the top-level scope does not track locals.
        let mut checker = ry_checker::Checker::new(&path);
        let (_, scope) = checker.check_with_scope(&file);

        let symbols = collect_symbols(&file.stmts, &text, Some(&scope));
        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DocumentSymbolResponse::Nested(symbols)))
        }
    }
}

impl Backend {
    async fn update_doc(&self, path: String, text: String) {
        let mut state = self.state.lock().await;
        state.docs.insert(path, text);
    }

    /// Re-check ALL open documents and publish diagnostics for the file
    /// identified by `uri`.
    ///
    /// PERFORMANCE: for each `didChange`, we rebuild a `Project` from
    /// ALL open documents and run the full three-pass check. For small
    /// workspaces (10-50 files) this is fast enough for interactive
    /// use. For very large workspaces, the per-keystroke cost may
    /// become noticeable; a future revision should add debouncing and
    /// incremental re-checking (only re-parse the file that changed).
    async fn publish_diagnostics(&self, uri: Url) {
        // Snapshot the open docs under the lock, then drop the lock
        // before running the checker so a slow check doesn't block
        // other LSP requests (e.g. didOpen of a second file).
        let (path, docs) = {
            let state = self.state.lock().await;
            (uri_to_path(&uri), state.docs.clone())
        };

        // Build a multi-file Project from every open document so
        // cross-file calls resolve.
        let mut project = Project::new();
        for (doc_path, doc_text) in &docs {
            let mut parser = match RParser::new() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("parser init failed: {}", e);
                    continue;
                }
            };
            let file = match parser.parse(doc_path, doc_text) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("parse {}: {}", doc_path, e);
                    continue;
                }
            };
            project.add_file(doc_path.clone(), file);
        }

        let per_file = project.check();
        // Look up the source text for the file we are diagnosing so we
        // can convert byte offsets to precise LSP ranges. We snapshot
        // it as a borrowed `&str` outside the iterator to avoid
        // re-borrowing on every diagnostic.
        let source_text = docs.get(&path).map(|s| s.as_str());
        // Parse inline suppression comments once for the target file
        // (`# ry: ignore`, `# noqa`, `# ry: ignore-file`) so the
        // per-diagnostic filter below is a cheap lookup.
        let (file_level, suppressions) = match source_text {
            Some(text) => {
                let supps = ry_checker::parse_suppressions(text);
                (ry_checker::has_file_suppression(text), supps)
            }
            None => (false, Vec::new()),
        };
        let diags_for_uri: Vec<LspDiagnostic> = per_file
            .into_iter()
            .filter(|(p, _)| p == &path)
            .flat_map(|(_, ds)| ds)
            // Drop diagnostics covered by inline suppression comments.
            .filter(|d| !file_level && !ry_checker::is_suppressed(d, &suppressions))
            .map(|d| match source_text {
                // Prefer the source-aware path so editors squiggle the
                // exact offending token instead of a single character.
                Some(text) => diagnostic_to_lsp_with_source(&d, text),
                // Fallback: source text missing (defensive — the file
                // was just open, so this branch should not normally
                // fire). Keep the old single-character behavior.
                None => diagnostic_to_lsp(d),
            })
            .collect();
        self.client
            .publish_diagnostics(uri, diags_for_uri, None)
            .await;
    }
}

/// Find the identifier (variable name) at a given line and column in
/// the source text. Returns `None` if the position is not on an
/// identifier-like character sequence. The search expands left and
/// right from the cursor to find the boundaries of the word.
///
/// This is a simple character-based scan, not a full parser query. It
/// handles the common case of hovering over a bare identifier like
/// `x`, `my_var`, `result`. It does not handle dotted access (`df$col`)
/// or function call syntax; those would require parser-level position
/// information.
fn find_identifier_at_position(text: &str, line: usize, col: usize) -> Option<String> {
    let line_str = text.lines().nth(line)?;
    let bytes = line_str.as_bytes();
    if bytes.is_empty() || col >= bytes.len() {
        return None;
    }
    // The character at the cursor must be identifier-like.
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.';
    if !is_ident_char(bytes[col]) {
        // Check if the cursor is just after an identifier (common when
        // the user places the cursor right at the end of a word).
        if col > 0 && is_ident_char(bytes[col - 1]) {
            // Expand from col-1 instead.
        } else {
            return None;
        }
    }
    // Expand left to find the start of the identifier.
    let mut start = col;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }
    // Expand right to find the end.
    let mut end = col;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }
    if start >= end {
        return None;
    }
    let ident = std::str::from_utf8(&bytes[start..end]).ok()?;
    // Filter out pure-number identifiers (123) and reserved words.
    if ident.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Filter out R keywords that are not variable bindings.
    if matches!(
        ident,
        "if" | "else" | "for" | "while" | "repeat" | "function" | "return"
            | "break" | "next" | "TRUE" | "FALSE" | "NULL" | "NA" | "Inf"
            | "NaN" | "in"
    ) {
        return None;
    }
    Some(ident.to_string())
}

/// Walk the AST of `file` looking for every definition site of `name`
/// and return each as an LSP `Location` (URI + range) inside `uri`.
///
/// A "definition site" is one of:
///   * an assignment whose target is a bare identifier (`x <- ...`),
///     which also covers the common R idiom `f <- function(...) ...`;
///   * a named function definition (`Stmt::FunctionDef { name: Some(..) }`).
///
/// The walk recurses into function bodies (both named and anonymous
/// `Expr::Function` literals), `if`/`for`/`while` blocks, and the
/// sub-expressions of calls, binary/unary ops, and index operations,
/// so that local definitions introduced inside nested scopes are
/// found as well. Each returned `Location`'s range covers exactly the
/// identifier characters so the editor places the cursor on the name.
fn find_definition_locations(file: &SourceFile, name: &str, uri: &Url) -> Vec<Location> {
    let mut spans: Vec<Span> = Vec::new();
    for stmt in &file.stmts {
        find_def_spans_in_stmt(stmt, name, &mut spans);
    }
    spans
        .into_iter()
        .map(|sp| span_to_location(sp, name, uri))
        .collect()
}

/// Convert a definition-site `Span` into an LSP `Location`. The range
/// runs from `(span.line, span.col)` to `(span.line, span.col +
/// name.len())`, i.e. it highlights the identifier itself. For ASCII
/// identifiers `name.len()` equals both the byte and char count; the
/// existing diagnostic conversion makes the same ASCII assumption (see
/// `diagnostic_to_lsp`).
fn span_to_location(span: Span, name: &str, uri: &Url) -> Location {
    let start = Position {
        line: span.line as u32,
        character: span.col as u32,
    };
    let end = Position {
        line: span.line as u32,
        character: span.col as u32 + name.len() as u32,
    };
    Location {
        uri: uri.clone(),
        range: Range { start, end },
    }
}

/// Recurse into a statement looking for definitions of `name`,
/// appending each definition's `Span` to `out`.
fn find_def_spans_in_stmt(stmt: &Stmt, name: &str, out: &mut Vec<Span>) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            // An assignment `x <- v` defines `x`. This also catches
            // `f <- function(..) ..` because the parser models named
            // function definitions as `Assign` with an `Expr::Function`
            // value.
            if let Expr::Ident { name: n, span } = target {
                if n == name {
                    out.push(*span);
                }
            }
            // The value may contain nested local definitions, e.g. an
            // anonymous function literal whose body assigns a local.
            find_def_spans_in_expr(value, name, out);
        }
        Stmt::FunctionDef {
            name: fn_name,
            body,
            span,
            ..
        } => {
            // Named function-definition statements (currently the
            // parser always emits `name: None`, but handle `Some`
            // for completeness / future grammar changes).
            if let Some(n) = fn_name {
                if n == name {
                    out.push(*span);
                }
            }
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Stmt::If { then, else_, .. } => {
            for s in then {
                find_def_spans_in_stmt(s, name, out);
            }
            if let Some(else_block) = else_ {
                for s in else_block {
                    find_def_spans_in_stmt(s, name, out);
                }
            }
        }
        Stmt::For { name: loop_var, body, span, .. } => {
            // The loop variable is a real binding in R (the checker
            // binds it at check_stmt). Record its definition so that
            // go-to-def on a reference to the loop variable works.
            if loop_var == name {
                out.push(*span);
            }
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Stmt::While { body, .. } => {
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                find_def_spans_in_expr(v, name, out);
            }
        }
        Stmt::Expr(e) => find_def_spans_in_expr(e, name, out),
    }
}

/// Recurse into an expression looking for nested statement bodies
/// (function literals, conditional expressions) that may contain
/// definitions of `name`. Operator/call/index operands are walked too
/// so that function literals nested inside them are discovered.
fn find_def_spans_in_expr(expr: &Expr, name: &str, out: &mut Vec<Span>) {
    match expr {
        Expr::Function { body, .. } => {
            for s in body {
                find_def_spans_in_stmt(s, name, out);
            }
        }
        Expr::If { then, else_, .. } => {
            find_def_spans_in_expr(then, name, out);
            if let Some(e) = else_ {
                find_def_spans_in_expr(e, name, out);
            }
        }
        Expr::Call { func, args, .. } => {
            find_def_spans_in_expr(func, name, out);
            for arg in args {
                find_def_spans_in_expr(&arg.value, name, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            find_def_spans_in_expr(lhs, name, out);
            find_def_spans_in_expr(rhs, name, out);
        }
        Expr::UnaryOp { expr, .. } => find_def_spans_in_expr(expr, name, out),
        Expr::Index { base, args, .. } => {
            find_def_spans_in_expr(base, name, out);
            for arg in args {
                find_def_spans_in_expr(&arg.value, name, out);
            }
        }
        // Literals, bare identifiers, NULL, NA, and Unknown carry no
        // nested statement bodies.
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
fn collect_symbols(stmts: &[Stmt], text: &str, scope: Option<&Scope>) -> Vec<DocumentSymbol> {
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
fn collect_from_stmt(stmt: &Stmt, text: &str, scope: Option<&Scope>, out: &mut Vec<DocumentSymbol>) {
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
            target, value, span, ..
        } => {
            // Only bare-identifier targets (`x <- ...`) become symbols.
            // Complex targets (`df$col <- 1`, `x[1] <- 2`) are skipped:
            // they don't introduce a new name in the outline.
            let Expr::Ident { name, span: ident_span } = target else {
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
        let mut children: Vec<DocumentSymbol> =
            params.iter().filter_map(param_to_symbol).collect();
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
    Some(
        if is_function {
            "function"
        } else {
            "variable"
        }
        .to_string(),
    )
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

/// Convert a full-statement `Span` to an LSP `Range` by computing the
/// end position from the span's byte offset against the source text.
/// The start uses the span's pre-resolved `line` / `col` (matching
/// `diagnostic_to_lsp`); the end is derived by counting newlines and
/// characters from the start of the file up to `span.end`.
fn span_to_range(text: &str, span: Span) -> Option<Range> {
    let start = Position {
        line: span.line as u32,
        character: span.col as u32,
    };
    let end = byte_offset_to_position(text, span.end);
    Some(Range { start, end })
}

/// Map a byte offset into the source text to an LSP `Position`
/// (0-indexed line, 0-indexed character column). Mirrors the
/// parser's `char_col` helper: each character advances the column,
/// each newline resets it and bumps the line.
///
/// UTF-16 NOTE: the LSP spec defines `Position.character` as a UTF-16
/// code-unit offset, not a Rust `char` count. This helper counts
/// `char`s (Unicode scalar values), which is identical to the UTF-16
/// count for the BMP subset that excludes astral-plane characters
/// (emoji, rare CJK). For pure ASCII source the two counts agree
/// exactly. For non-ASCII content this is an approximation; a future
/// revision should compute true UTF-16 offsets for full correctness.
fn byte_offset_to_position(text: &str, byte_offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (b, ch) in text.char_indices() {
        if b >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position { line, character: col }
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

/// Convert a `file://` URI to a filesystem path string. Falls back to
/// the URI's string form when the URI isn't a `file:` scheme (so a
/// virtual or untitled document still gets a stable key).
fn uri_to_path(uri: &Url) -> String {
    uri.to_file_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| uri.as_str().to_string())
}

/// Convert a `ry_checker::Diagnostic` to an LSP `Diagnostic`.
///
/// ry's `Span` carries pre-resolved 0-indexed `line` and 0-indexed
/// `col` (in char count). LSP positions are 0-indexed lines but
/// UTF-16 code units for the character offset. For ASCII files the two
/// are identical; for non-ASCII content this v1 conversion is an
/// approximation (we forward `col` unchanged). The end position is set
/// to a single-character range anchored at the start, which editors
/// render as a squiggle under one character. A future revision should
/// compute a precise range from the span's byte offsets.
fn diagnostic_to_lsp(d: RyDiagnostic) -> LspDiagnostic {
    let start = Position {
        line: d.span.line as u32,
        character: d.span.col as u32,
    };
    let end = Position {
        line: d.span.line as u32,
        // Single-character range so the squiggle is non-empty even for
        // zero-width spans. Future revision should use span.start/end
        // against the source text for a precise range.
        character: (d.span.col as u32) + 1,
    };
    let severity = match d.severity {
        Severity::Error => Some(DiagnosticSeverity::ERROR),
        Severity::Warning => Some(DiagnosticSeverity::WARNING),
        Severity::Info => Some(DiagnosticSeverity::INFORMATION),
    };
    LspDiagnostic {
        range: Range { start, end },
        severity,
        code: Some(NumberOrString::String(d.code.to_string())),
        source: Some("ry".to_string()),
        message: d.message,
        ..Default::default()
    }
}

/// Convert a `ry_checker::Diagnostic` to an LSP `Diagnostic` using a
/// precise multi-character range derived from the span's byte offsets
/// against the source text.
///
/// Unlike `diagnostic_to_lsp` (which falls back to a single-character
/// range anchored at the span's pre-resolved `line` / `col`), this
/// version maps both `span.start` and `span.end` byte offsets to LSP
/// `Position`s via `byte_offset_to_position`, so editors squiggle
/// exactly the offending token. If the span is zero-width
/// (`start == end`), we extend the end by one character so the
/// squiggle is still visible.
///
/// This is the path used by `publish_diagnostics`; the older
/// `diagnostic_to_lsp` is retained as a fallback for tests and for the
/// rare case where source text is unavailable.
fn diagnostic_to_lsp_with_source(d: &RyDiagnostic, text: &str) -> LspDiagnostic {
    let start = byte_offset_to_position(text, d.span.start);
    let end = byte_offset_to_position(text, d.span.end);
    // Zero-width spans (start == end) appear in the AST for some
    // synthetic sites. Extend by one character so the editor renders a
    // non-empty squiggle, mirroring `diagnostic_to_lsp`'s behavior.
    let end = if start == end {
        Position {
            line: start.line,
            character: start.character + 1,
        }
    } else {
        end
    };
    let severity = match d.severity {
        Severity::Error => Some(DiagnosticSeverity::ERROR),
        Severity::Warning => Some(DiagnosticSeverity::WARNING),
        Severity::Info => Some(DiagnosticSeverity::INFORMATION),
    };
    LspDiagnostic {
        range: Range { start, end },
        severity,
        code: Some(NumberOrString::String(d.code.to_string())),
        source: Some("ry".to_string()),
        message: d.message.clone(),
        ..Default::default()
    }
}

/// Entry point for the LSP server. Reads from stdin, writes to stdout.
///
/// IMPORTANT: the caller (the CLI) MUST install a `tracing_subscriber`
/// that routes output to stderr BEFORE calling this function. Any log
/// output on stdout will corrupt the JSON-RPC stream and break the
/// client. See `crates/ry-cli/src/main.rs`'s `Cmd::Server` arm.
pub async fn run() -> LspResult<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::build(|client| Backend {
        client,
        state: Arc::new(Mutex::new(State::default())),
    })
    .finish();
    Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_checker::Diagnostic;
    use ry_core::Span;

    #[test]
    fn converts_error_diagnostic() {
        let d = Diagnostic::new(
            Severity::Error,
            Span::new(0, 5, 2, 4),
            "test.R",
            "RY040",
            "test message",
        );
        let lsp = diagnostic_to_lsp(d);
        assert_eq!(lsp.range.start.line, 2);
        assert_eq!(lsp.range.start.character, 4);
        // Single-character range: end character is start + 1.
        assert_eq!(lsp.range.end.character, 5);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp.message, "test message");
        assert_eq!(lsp.source.as_deref(), Some("ry"));
        match lsp.code {
            Some(NumberOrString::String(s)) => assert_eq!(s, "RY040"),
            other => panic!("expected String code, got {:?}", other),
        }
    }

    #[test]
    fn converts_warning_diagnostic() {
        let d = Diagnostic::new(
            Severity::Warning,
            Span::new(0, 5, 0, 0),
            "test.R",
            "RY001",
            "warning",
        );
        let lsp = diagnostic_to_lsp(d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn multi_char_range_from_source() {
        // The source-aware converter must produce a precise multi-char
        // range from the span's byte offsets rather than the old
        // single-character fallback.
        let text = "x <- 1L + \"hello\"\n";
        // The RY040 diagnostic for `+` should span exactly the `+`
        // operator at byte offsets 7..8 (line 0, col 7).
        let d = Diagnostic::new(
            Severity::Error,
            Span::new(7, 8, 0, 7),
            "test.R",
            "RY040",
            "test",
        );
        let lsp = diagnostic_to_lsp_with_source(&d, text);
        assert_eq!(lsp.range.start.line, 0);
        assert_eq!(lsp.range.start.character, 7);
        assert_eq!(lsp.range.end.line, 0);
        assert_eq!(lsp.range.end.character, 8);
        // Non-range fields must still be populated identically to the
        // fallback path so behavior is unchanged except for the range.
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(lsp.message, "test");
        assert_eq!(lsp.source.as_deref(), Some("ry"));
        match lsp.code {
            Some(NumberOrString::String(s)) => assert_eq!(s, "RY040"),
            other => panic!("expected String code, got {:?}", other),
        }
    }

    #[test]
    fn zero_width_span_extends_by_one_char() {
        // A zero-width span (start == end) must be widened by exactly
        // one character so the squiggle is non-empty in the editor.
        let text = "x <- 1L\n";
        let d = Diagnostic::new(
            Severity::Error,
            Span::new(0, 0, 0, 0),
            "test.R",
            "RY040",
            "test",
        );
        let lsp = diagnostic_to_lsp_with_source(&d, text);
        assert_eq!(lsp.range.start.line, 0);
        assert_eq!(lsp.range.start.character, 0);
        assert_eq!(lsp.range.end.line, 0);
        assert_eq!(lsp.range.end.character, 1);
    }

    #[test]
    fn multi_char_range_on_second_line() {
        // Byte offsets that cross a newline must land on the correct
        // line and column. Here the diagnostic sits on line 1 of a
        // two-line source.
        let text = "x <- 1L\ny <- 2L\n";
        // The `y` identifier is at byte offset 8 (the byte right after
        // the first `\n`). It is one character wide.
        let d = Diagnostic::new(
            Severity::Warning,
            Span::new(8, 9, 1, 0),
            "test.R",
            "RY001",
            "warning",
        );
        let lsp = diagnostic_to_lsp_with_source(&d, text);
        assert_eq!(lsp.range.start.line, 1);
        assert_eq!(lsp.range.start.character, 0);
        assert_eq!(lsp.range.end.line, 1);
        assert_eq!(lsp.range.end.character, 1);
    }

    #[test]
    fn multi_char_range_spans_identifier() {
        // A diagnostic covering a multi-character identifier must
        // squiggle exactly the identifier's bytes.
        let text = "my_var <- 1L\n";
        // `my_var` occupies bytes 0..6.
        let d = Diagnostic::new(
            Severity::Info,
            Span::new(0, 6, 0, 0),
            "test.R",
            "RY001",
            "info",
        );
        let lsp = diagnostic_to_lsp_with_source(&d, text);
        assert_eq!(lsp.range.start.line, 0);
        assert_eq!(lsp.range.start.character, 0);
        assert_eq!(lsp.range.end.line, 0);
        assert_eq!(lsp.range.end.character, 6);
    }

    #[test]
    fn converts_info_diagnostic() {
        let d = Diagnostic::new(
            Severity::Info,
            Span::new(0, 5, 1, 2),
            "test.R",
            "RY001",
            "info",
        );
        let lsp = diagnostic_to_lsp(d);
        assert_eq!(lsp.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn uri_to_path_handles_file_scheme() {
        let uri = Url::parse("file:///tmp/foo.R").unwrap();
        let path = uri_to_path(&uri);
        assert!(path.ends_with("foo.R"), "path was {}", path);
    }

    #[test]
    fn uri_to_path_falls_back_for_non_file_scheme() {
        // An `untitled:` URI can't be turned into a file path; we fall
        // back to the URI string so the document still has a stable
        // identity in the open-docs map.
        let uri = Url::parse("untitled:Untitled-1").unwrap();
        let path = uri_to_path(&uri);
        assert_eq!(path, "untitled:Untitled-1");
    }

    #[test]
    fn find_identifier_middle_of_word() {
        let text = "x <- 42\nresult <- x + 1\n";
        // Hover over 's' in 'result' (line 1, col 2)
        let ident = find_identifier_at_position(text, 1, 2);
        assert_eq!(ident.as_deref(), Some("result"));
    }

    #[test]
    fn find_identifier_start_of_word() {
        let text = "my_var <- 1L\n";
        let ident = find_identifier_at_position(text, 0, 0);
        assert_eq!(ident.as_deref(), Some("my_var"));
    }

    #[test]
    fn find_identifier_end_of_word() {
        let text = "my_var <- 1L\n";
        // Cursor right after the 'r' (col 6, which is the space)
        let ident = find_identifier_at_position(text, 0, 6);
        assert_eq!(ident.as_deref(), Some("my_var"));
    }

    #[test]
    fn find_identifier_on_operator_returns_none() {
        let text = "x <- 1L\n";
        // Hover over the '<' operator
        let ident = find_identifier_at_position(text, 0, 2);
        assert_eq!(ident, None);
    }

    #[test]
    fn find_identifier_filters_keywords() {
        let text = "if (TRUE) { x <- 1 }\n";
        let ident = find_identifier_at_position(text, 0, 1);
        assert_eq!(ident, None, "keywords should not be identifiers");
    }

    #[test]
    fn find_identifier_filters_numbers() {
        let text = "x <- 123\n";
        let ident = find_identifier_at_position(text, 0, 5);
        assert_eq!(ident, None, "pure numbers should not be identifiers");
    }

    #[test]
    fn hover_returns_type_for_known_variable() {
        // Integration test: parse a simple R snippet, check it, and
        // verify that hover on a variable returns its type.
        let text = "x <- 1L + 2L\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", text).unwrap();
        let mut checker = ry_checker::Checker::new("test.R");
        let (_, scope) = checker.check_with_scope(&file);
        let t = scope.get("x").expect("x should be in scope");
        assert_eq!(t.mode, ry_core::types::Mode::Integer);
    }

    #[test]
    fn goto_def_finds_variable_assignment() {
        // `x <- 1L + 2L` defines `x` at line 0, col 0 (1-char name).
        let text = "x <- 1L + 2L\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", text).unwrap();
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let locs = find_definition_locations(&file, "x", &uri);
        assert_eq!(locs.len(), 1, "expected exactly one definition of x");
        let loc = &locs[0];
        assert_eq!(loc.uri, uri);
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 0);
        // Name "x" is one character wide.
        assert_eq!(loc.range.end.line, 0);
        assert_eq!(loc.range.end.character, 1);
    }

    #[test]
    fn goto_def_finds_function_definition() {
        // `add <- function(a, b) a + b` defines `add` (3 chars) at
        // line 0, col 0. The parser models this as an Assign whose
        // value is an Expr::Function, so the Assign-target branch of
        // the walk must find it.
        let text = "add <- function(a, b) a + b\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", text).unwrap();
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let locs = find_definition_locations(&file, "add", &uri);
        assert_eq!(locs.len(), 1, "expected exactly one definition of add");
        let loc = &locs[0];
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 0);
        assert_eq!(loc.range.end.character, 3, "add is 3 chars wide");
    }

    #[test]
    fn goto_def_finds_local_definition_inside_function_body() {
        // A local assignment nested inside a function literal must be
        // found by recursing through Expr::Function -> body. `local`
        // sits on line 1, indented 2 spaces.
        let text = "f <- function() {\n  local <- 1L\n  local\n}\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", text).unwrap();
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let locs = find_definition_locations(&file, "local", &uri);
        assert_eq!(locs.len(), 1, "expected exactly one definition of local");
        let loc = &locs[0];
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(loc.range.start.character, 2);
        assert_eq!(loc.range.end.character, 2 + "local".len() as u32);
    }

    #[test]
    fn goto_def_finds_reassignment_as_multiple_locations() {
        // Two assignments to the same name yield two Locations; the
        // editor can present them as alternatives.
        let text = "x <- 1L\nx <- 2L\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", text).unwrap();
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let locs = find_definition_locations(&file, "x", &uri);
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].range.start.line, 0);
        assert_eq!(locs[1].range.start.line, 1);
    }

    #[test]
    fn goto_def_returns_empty_for_undefined_name() {
        let text = "x <- 1L\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", text).unwrap();
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let locs = find_definition_locations(&file, "does_not_exist", &uri);
        assert!(locs.is_empty(), "expected no definitions");
    }

    // ---- documentSymbol helpers ----

    /// Helper: parse + check a snippet and return its top-level
    /// `DocumentSymbol`s. Mirrors what the `document_symbol` LSP method
    /// does, minus the async state lookup.
    fn doc_symbols(src: &str) -> Vec<DocumentSymbol> {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let mut checker = ry_checker::Checker::new("test.R");
        let (_, scope) = checker.check_with_scope(&file);
        collect_symbols(&file.stmts, src, Some(&scope))
    }

    #[test]
    fn document_symbols_for_mixed_top_level_bindings() {
        // The canonical example from the task: a function, a call
        // result, and a string. We expect 3 top-level symbols with the
        // right names and kinds.
        let src = "add <- function(x = 0, y = 0) { x + y }\nresult <- add(1, 2)\nname <- \"hello\"\n";
        let symbols = doc_symbols(src);
        assert_eq!(symbols.len(), 3, "got {:?}", symbols);

        assert_eq!(symbols[0].name, "add");
        assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
        // The checker infers a function type for `add`, so the detail
        // surfaces that (which starts with "function"). We don't pin
        // the exact signature since return-type inference may refine
        // it over time; we just check it identifies a function.
        let detail = symbols[0].detail.as_deref().expect("add should have detail");
        assert!(
            detail.starts_with("function"),
            "expected detail to start with 'function', got: {}",
            detail
        );

        assert_eq!(symbols[1].name, "result");
        assert_eq!(symbols[1].kind, SymbolKind::VARIABLE);

        assert_eq!(symbols[2].name, "name");
        assert_eq!(symbols[2].kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn document_symbols_detail_uses_inferred_type() {
        // The checker infers `x` as a scalar integer, so the detail
        // string must mention "integer".
        let src = "x <- 1L + 2L\n";
        let symbols = doc_symbols(src);
        assert_eq!(symbols.len(), 1);
        let detail = symbols[0].detail.as_deref().expect("detail should be set");
        assert!(
            detail.contains("integer"),
            "expected integer in detail, got: {}",
            detail
        );
    }

    #[test]
    fn document_symbols_function_has_nested_children() {
        // A function literal assigned to `f` contains a nested local
        // function `g`. `g` must appear as a child of `f` (not at the
        // top level), and `g` itself must be classified as a function.
        let src = "f <- function() {\n  g <- function() { 1L }\n  g\n}\n";
        let symbols = doc_symbols(src);
        assert_eq!(symbols.len(), 1);
        let f = &symbols[0];
        assert_eq!(f.name, "f");
        assert_eq!(f.kind, SymbolKind::FUNCTION);
        let children = f
            .children
            .as_ref()
            .expect("f should have nested children from its body");
        let g = children
            .iter()
            .find(|c| c.name == "g")
            .expect("should find nested g");
        assert_eq!(g.kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn document_symbols_selection_range_covers_identifier() {
        // For `my_var <- 42`, the selection range must cover exactly
        // the 6-character identifier at the start of line 0, and it
        // must be contained within the enclosing range.
        let src = "my_var <- 42\n";
        let symbols = doc_symbols(src);
        assert_eq!(symbols.len(), 1);
        let sym = &symbols[0];
        assert_eq!(sym.name, "my_var");
        assert_eq!(sym.selection_range.start.line, 0);
        assert_eq!(sym.selection_range.start.character, 0);
        assert_eq!(sym.selection_range.end.line, 0);
        assert_eq!(sym.selection_range.end.character, "my_var".len() as u32);
        // selection_range must be inside range per the LSP spec.
        assert!(sym.range.start <= sym.selection_range.start);
        assert!(sym.range.end >= sym.selection_range.end);
    }

    #[test]
    fn document_symbols_empty_for_no_bindings() {
        // A bare expression with no assignments yields no symbols.
        let src = "1L + 2L\n";
        let symbols = doc_symbols(src);
        assert!(symbols.is_empty(), "expected no symbols, got {:?}", symbols);
    }

    #[test]
    fn document_symbols_flatten_control_flow_bodies() {
        // Bindings inside `if` / `for` blocks are visible in R's
        // enclosing scope, so they should surface at the current
        // outline level rather than disappearing.
        let src = "if (TRUE) {\n  a <- 1L\n}\nfor (i in 1:3) {\n  b <- 2L\n}\n";
        let symbols = doc_symbols(src);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"), "a should be in outline: {:?}", names);
        assert!(names.contains(&"b"), "b should be in outline: {:?}", names);
    }
}
