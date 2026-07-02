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
//!   * `textDocument/references` (find all usages of a symbol across open files)
//!   * `textDocument/documentSymbol` (outline view of the file's bindings)
//!   * `workspace/symbol` (search for symbols across all open files)
//!   * `textDocument/rename` (workspace-wide rename of a variable / function)
//!   * `textDocument/prepareRename` (validates the cursor is on a renameable identifier)
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
use ry_core::{Expr, Mode, RParser, SourceFile, Span, Stmt};
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
        state.root = params.root_uri.and_then(|uri| uri.to_file_path().ok());
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
                // Enable `textDocument/references` so the client can
                // find all usages of a variable / function across the
                // workspace (Shift+F12 / "Find All References"). The
                // handler is `references` below; it walks every open
                // document's AST collecting matching `Expr::Ident`
                // nodes, optionally including the definition site.
                references_provider: Some(OneOf::Left(true)),
                // Enable `textDocument/documentSymbol` so the client can
                // render an outline of the file's structure (functions,
                // variables) in the sidebar. The handler is
                // `document_symbol` below.
                document_symbol_provider: Some(OneOf::Left(true)),
                // Enable `textDocument/inlayHint` so the client can
                // request inline "ghost text" annotations showing the
                // inferred type of each binding. For a checker with no
                // annotation syntax (like R), this is the primary way
                // users see the checker's work. The handler is
                // `inlay_hint` below.
                inlay_hint_provider: Some(OneOf::Left(true)),
                // Enable `textDocument/completion` so editors can
                // auto-complete variable / function names from the
                // checked scope, and column names after a `$` trigger.
                // The `:` trigger is advertised in anticipation of
                // future `package::name` namespace completion; v1 has
                // no special handling for it and it falls through to
                // the generic in-scope list. The handler is
                // `completion` below.
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["$".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                // Enable `textDocument/signatureHelp` so editors can
                // show function parameter hints when the user types
                // `(` or `,` inside a call. The handler is
                // `signature_help` below; it walks backward from the
                // cursor to identify the enclosing call, looks up the
                // function's parameter names in a small curated table,
                // and returns a `SignatureHelp` highlighting the
                // active parameter (counted by commas).
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    ..Default::default()
                }),
                // Enable `workspace/symbol` so the client can search
                // for symbols across all open files (Ctrl+T / "Go to
                // Symbol in Workspace"). The handler is `symbol`
                // below; it walks every open document's AST, flattens
                // the hierarchical `DocumentSymbol` tree produced by
                // `collect_symbols` into a flat list of
                // `SymbolInformation` (each carrying its file `Url`),
                // and filters by a case-insensitive substring match
                // against the query string.
                workspace_symbol_provider: Some(OneOf::Left(true)),
                // Enable `textDocument/rename` so the client can do a
                // workspace-wide rename of a variable / function
                // (F2 / "Rename Symbol"). The handler is `rename`
                // below; it reuses the references walker to find every
                // occurrence of the identifier at the cursor across all
                // open documents and produces a `WorkspaceEdit`
                // grouping `TextEdit`s by file URI.
                //
                // `prepare_provider: true` also advertises
                // `textDocument/prepareRename` (handled by
                // `prepare_rename` below) so the editor can validate
                // that the cursor sits on a renameable identifier
                // before showing the rename UI.
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                // Enable `textDocument/documentHighlight` so the client
                // can highlight all in-file occurrences of the symbol
                // under the cursor (e.g. with a colored background). The
                // handler is `document_highlight` below; it reuses the
                // reference walker to find every `Expr::Ident` matching
                // the cursor's identifier in the current file, classifying
                // assignment targets as `WRITE` and all other occurrences
                // as `READ`.
                document_highlight_provider: Some(OneOf::Left(true)),
                // Enable `textDocument/foldingRange` so editors can offer
                // code folding (collapsible regions) for multi-line
                // function bodies, `if`/`else` blocks, and `for`/`while`
                // loop bodies. The handler is `folding_range` below; it
                // walks the AST looking for statement spans that cross a
                // newline and emits one `FoldingRange` per such span.
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                // Enable `textDocument/codeAction` so editors can offer
                // quick fixes for diagnostics. The handler is
                // `code_action` below; it offers per-diagnostic
                // `# ry: ignore[CODE]` line-suppression comments and a
                // file-level `# ry: ignore-file` action.
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                // Enable `textDocument/selectionRange` so editors can
                // offer expand/shrink selection ("Expand Selection" /
                // "Shrink Selection") based on AST structure. The
                // handler is `selection_range` below; it builds a chain
                // of progressively wider ranges (identifier ->
                // enclosing statement -> whole file) for each cursor
                // position requested.
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
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
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
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
        let identifier =
            find_identifier_at_position(&text, position.line as usize, position.character as usize);
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
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
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

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        // Snapshot ALL open documents under the lock, then drop the
        // lock before parsing/walking so a slow search doesn't block
        // other LSP requests. References are workspace-wide, so we
        // search every open document (not just the current one).
        let docs = {
            let state = self.state.lock().await;
            state.docs.clone()
        };

        let text = docs.get(&path).cloned();
        let Some(text) = text else {
            return Ok(None);
        };

        // Reuse the same word-finding helper as `hover` /
        // `goto_definition` to extract the identifier under the
        // cursor. Returns `None` (no references) for operators,
        // numbers, and keywords.
        let identifier =
            find_identifier_at_position(&text, position.line as usize, position.character as usize);
        let Some(identifier) = identifier else {
            return Ok(None);
        };

        // A single parser instance is reused across all documents;
        // tree-sitter's `Parser` is designed to be reused and this
        // avoids the per-file allocation cost of `publish_diagnostics`'s
        // pattern.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let mut all_locations = Vec::new();
        for (doc_path, doc_text) in &docs {
            let file = match parser.parse(doc_path, doc_text) {
                Ok(f) => f,
                // Skip documents that fail to parse rather than
                // aborting the whole search; a syntax error in one
                // file shouldn't hide references in another.
                Err(_) => continue,
            };
            let doc_uri = path_to_uri(doc_path);
            let locs = find_references_in_file(
                &file,
                &identifier,
                &doc_uri,
                doc_text,
                include_declaration,
            );
            all_locations.extend(locs);
        }

        if all_locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(all_locations))
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

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let range = params.range;

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Parse the document. On any parse failure we return `None`
        // (no hints) rather than erroring, so the editor simply shows
        // nothing instead of a broken state. Mirrors `document_symbol`.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let file = match parser.parse(&path, &text) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        // Run the checker so we can attach inferred types to each
        // binding. The top-level scope maps binding names to their
        // `RType`; nested scopes (function bodies) are not tracked by
        // the top-level scope, so locals fall back to whatever the
        // scope exposes (typically nothing, which yields no hint).
        let mut checker = ry_checker::Checker::new(&path);
        let (_, scope) = checker.check_with_scope(&file);

        let mut hints = collect_inlay_hints(&file, &scope, &text);
        // Filter to the visible range the editor requested. Hints
        // outside `[range.start, range.end]` are dropped so we don't
        // waste client render cycles on off-screen annotations.
        hints.retain(|h| {
            let within_start = h.position.line > range.start.line
                || (h.position.line == range.start.line
                    && h.position.character >= range.start.character);
            let within_end = h.position.line < range.end.line
                || (h.position.line == range.end.line
                    && h.position.character <= range.end.character);
            within_start && within_end
        });
        if hints.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hints))
        }
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let position = params.text_document_position.position;

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Parse and check the file to get the scope. Mirrors `hover`
        // and `inlay_hint`: on any parse failure we return `None`
        // (no completions) rather than erroring, so the editor simply
        // shows nothing instead of a broken state.
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

        let items = collect_completions(&text, position, &params.context, &scope);
        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let path = uri_to_path(&uri);
        let position = params.text_document_position_params.position;

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Walk backward from the cursor on the current line to find
        // the enclosing call's function name and the active parameter
        // index. Returns `None` when the cursor is not inside a call
        // (e.g. at the top level, inside `[`, or before any `(`).
        let (func_name, active_param) =
            match find_enclosing_call(&text, position.line as usize, position.character as usize) {
                Some(c) => c,
                None => return Ok(None),
            };

        // Look up the function's parameter names. We only support
        // base-R functions from the curated table; user-defined
        // functions would require reaching into the checker's FnTable
        // from the LSP crate, which is out of scope for v1.
        let Some(params_list) = get_signature(&func_name) else {
            return Ok(None);
        };

        // Build the signature label like `round(x, digits)` and the
        // per-parameter `ParameterInformation` list. The active
        // parameter (highlighted by the editor) is clamped to the
        // parameter count; if the user has typed more commas than
        // there are formal parameters, we return `None` so the editor
        // clears the popup rather than highlighting a non-existent
        // parameter.
        let active_param = if active_param < params_list.len() {
            Some(active_param as u32)
        } else {
            None
        };
        let label = format!("{}({})", func_name, params_list.join(", "));
        let param_infos: Vec<ParameterInformation> = params_list
            .iter()
            .map(|p| ParameterInformation {
                label: ParameterLabel::Simple(p.clone()),
                documentation: None,
            })
            .collect();

        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label,
                documentation: None,
                parameters: Some(param_infos),
                active_parameter: active_param,
            }],
            active_signature: Some(0),
            active_parameter: active_param,
        }))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> LspResult<Option<Vec<SymbolInformation>>> {
        let query = params.query;

        // Snapshot ALL open documents under the lock, then drop the
        // lock before parsing/walking so a slow search doesn't block
        // other LSP requests. Workspace symbols span every open
        // document, mirroring how `references` works.
        let docs = {
            let state = self.state.lock().await;
            state.docs.clone()
        };

        // A single parser instance is reused across all documents;
        // tree-sitter's `Parser` is designed to be reused.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        let mut all_symbols: Vec<SymbolInformation> = Vec::new();
        for (doc_path, doc_text) in &docs {
            let file = match parser.parse(doc_path, doc_text) {
                Ok(f) => f,
                // Skip documents that fail to parse rather than
                // aborting the whole search; a syntax error in one
                // file shouldn't hide symbols in another.
                Err(_) => continue,
            };
            // Run the checker so the inferred types are available to
            // the symbol detail strings via `collect_symbols`. The
            // resulting `SymbolInformation`s do not surface `detail`
            // (the LSP type has no such field), but the kind
            // classification (FUNCTION vs VARIABLE) does depend on
            // the AST shape, which the checker doesn't change.
            let mut checker = ry_checker::Checker::new(doc_path);
            let (_, scope) = checker.check_with_scope(&file);

            let doc_symbols = collect_symbols(&file.stmts, doc_text, Some(&scope));
            let doc_uri = path_to_uri(doc_path);
            // Flatten the hierarchical `DocumentSymbol` tree (which
            // nests function-body bindings as children) into a flat
            // list of `SymbolInformation`, attaching the file URI to
            // each symbol's `Location`. Workspace symbols is a flat
            // list per the LSP spec.
            all_symbols.extend(flatten_symbols_to_symbol_info(doc_symbols, &doc_uri));
        }

        // Filter by the query string (case-insensitive substring match
        // on the symbol name). An empty query returns every symbol,
        // matching the convention used by other LSP servers (the
        // editor typically caps the result count client-side).
        if !query.is_empty() {
            let query_lower = query.to_lowercase();
            all_symbols.retain(|s| s.name.to_lowercase().contains(&query_lower));
        }

        if all_symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(all_symbols))
        }
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        // Snapshot ALL open documents under the lock, then drop the
        // lock before parsing/walking so a slow rename doesn't block
        // other LSP requests. Rename is workspace-wide, so we walk
        // every open document (not just the current one).
        let docs = {
            let state = self.state.lock().await;
            state.docs.clone()
        };

        let text = docs.get(&path).cloned();
        let Some(text) = text else {
            return Ok(None);
        };

        // Find the identifier at the cursor position to learn the
        // old name. We rename ALL occurrences of that name across all
        // open documents, mirroring how `references` works. Returns
        // `None` (no rename) for operators, numbers, and keywords.
        let old_name =
            find_identifier_at_position(&text, position.line as usize, position.character as usize);
        let Some(old_name) = old_name else {
            return Ok(None);
        };

        // A single parser instance is reused across all documents.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };

        // Build the per-URI edit map. For each open document we find
        // every occurrence of `old_name` (including declaration sites,
        // since a rename must update the definition too) and append a
        // `TextEdit` replacing the old name with the new one. Edits
        // are grouped by file URI into the `WorkspaceEdit.changes`
        // map; the editor applies each group atomically per file.
        //
        // The same loop logic is factored into `build_rename_edits`
        // (used by the unit tests). We inline it here rather than
        // share the helper because the helper borrows pre-parsed
        // `SourceFile`s, while here we parse lazily inside the loop
        // and skip parse failures per-document.
        let mut edits: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for (doc_path, doc_text) in &docs {
            let file = match parser.parse(doc_path, doc_text) {
                Ok(f) => f,
                // Skip documents that fail to parse rather than
                // aborting the whole rename; a syntax error in one
                // file shouldn't block renaming in another.
                Err(_) => continue,
            };
            let doc_uri = path_to_uri(doc_path);
            // include_declaration = true: a rename must rewrite the
            // definition site as well as every read / call site.
            let locations = find_references_in_file(&file, &old_name, &doc_uri, doc_text, true);
            for loc in locations {
                edits.entry(doc_uri.clone()).or_default().push(TextEdit {
                    range: loc.range,
                    new_text: new_name.clone(),
                });
            }
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(edits),
            ..Default::default()
        }))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri.clone();
        let _ = uri; // retained for symmetry with other handlers
        let path = uri_to_path(&uri);
        let position = params.position;

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Validate that the cursor is on a renameable identifier
        // before the editor shows the rename UI. We reuse the same
        // word-finding helper as `hover` / `goto_definition`, but in
        // its range-returning variant so we can hand the editor the
        // exact span to highlight as the rename target. Returns
        // `None` (rename not allowed here) for operators, numbers,
        // keywords, and whitespace.
        let (_, range) = match find_identifier_range_at_position(
            &text,
            position.line as usize,
            position.character as usize,
        ) {
            Some(r) => r,
            None => return Ok(None),
        };

        Ok(Some(PrepareRenameResponse::Range(range)))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> LspResult<Option<Vec<DocumentHighlight>>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let path = uri_to_path(&uri);
        let position = params.text_document_position_params.position;

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Reuse the same word-finding helper as `hover` /
        // `references` to extract the identifier under the cursor.
        // Returns `None` (no highlights) for operators, numbers, and
        // keywords.
        let identifier =
            find_identifier_at_position(&text, position.line as usize, position.character as usize);
        let Some(identifier) = identifier else {
            return Ok(None);
        };

        // Parse the current document. Document highlight is scoped to
        // the current file (per the LSP spec), so we only parse once.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let file = match parser.parse(&path, &text) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        let highlights = collect_document_highlights(&file, &identifier, &text);
        if highlights.is_empty() {
            Ok(None)
        } else {
            Ok(Some(highlights))
        }
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
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
        // (no folding ranges) rather than erroring, so the editor
        // simply shows no fold markers instead of a broken state.
        // Mirrors `document_symbol` / `inlay_hint`.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let file = match parser.parse(&path, &text) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        let ranges = collect_folding_ranges(&file, &text);
        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Build one quick-fix per diagnostic currently visible at the
        // cursor (the client populates `params.context.diagnostics`
        // with the squiggles overlapping `params.range`). Each
        // per-diagnostic action appends a `# ry: ignore[CODE]`
        // suppression comment to the end of the offending line. When
        // a line already carries an ignore comment we skip it so the
        // lightbulb does not offer a redundant no-op.
        let mut actions: CodeActionResponse = Vec::new();
        for diag in &params.context.diagnostics {
            if let Some(action) = make_ignore_action(&uri, diag, &text) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        // The file-level action inserts `# ry: ignore-file` at line 0.
        // It is only offered when the file does not already carry a
        // file-level suppression, so the user never sees a duplicate.
        if let Some(action) = make_ignore_file_action(&uri, &text) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> LspResult<Option<Vec<SelectionRange>>> {
        let uri = params.text_document.uri.clone();
        let _ = uri; // retained for symmetry with the other handlers
        let path = uri_to_path(&uri);

        let text = {
            let state = self.state.lock().await;
            state.docs.get(&path).cloned()
        };

        let Some(text) = text else {
            return Ok(None);
        };

        // Parse the document. On any parse failure we return `None`
        // (no selection ranges) rather than erroring, so the editor
        // simply disables expand/shrink selection instead of showing
        // a broken state. Mirrors `document_symbol` / `folding_range`.
        let mut parser = match RParser::new() {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let file = match parser.parse(&path, &text) {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        // Build one `SelectionRange` chain per requested position.
        // The LSP spec allows the client to pass multiple cursor
        // positions in a single request (e.g. multi-cursor edit);
        // we return one chain per position in the same order.
        let ranges: Vec<SelectionRange> = params
            .positions
            .into_iter()
            .map(|pos| build_selection_range(pos, &file, &text))
            .collect();

        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
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
/// This is a thin wrapper around `find_identifier_range_at_position`
/// that discards the range; callers that need the identifier's span
/// (e.g. `prepare_rename`) should call the range-returning helper
/// directly.
///
/// This is a simple character-based scan, not a full parser query. It
/// handles the common case of hovering over a bare identifier like
/// `x`, `my_var`, `result`. It does not handle dotted access (`df$col`)
/// or function call syntax; those would require parser-level position
/// information.
fn find_identifier_at_position(text: &str, line: usize, col: usize) -> Option<String> {
    find_identifier_range_at_position(text, line, col).map(|(name, _)| name)
}

/// Find the identifier (variable name) at a given line and column in
/// the source text, returning BOTH the identifier string AND its LSP
/// `Range` (line + character offsets of the identifier span). Returns
/// `None` if the position is not on an identifier-like character
/// sequence.
///
/// The search expands left and right from the cursor to find the
/// boundaries of the word. The same filtering rules as
/// `find_identifier_at_position` apply: pure numbers and R keywords
/// are rejected (they are not renameable bindings).
///
/// Used by `prepare_rename` to validate that the cursor sits on a
/// renameable identifier before the editor shows the rename UI, and
/// to hand the editor the exact span to highlight as the rename
/// target.
fn find_identifier_range_at_position(
    text: &str,
    line: usize,
    col: usize,
) -> Option<(String, Range)> {
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
        "if" | "else"
            | "for"
            | "while"
            | "repeat"
            | "function"
            | "return"
            | "break"
            | "next"
            | "TRUE"
            | "FALSE"
            | "NULL"
            | "NA"
            | "Inf"
            | "NaN"
            | "in"
    ) {
        return None;
    }
    let range = Range {
        start: Position {
            line: line as u32,
            character: start as u32,
        },
        end: Position {
            line: line as u32,
            character: end as u32,
        },
    };
    Some((ident.to_string(), range))
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
        Stmt::For {
            name: loop_var,
            body,
            span,
            ..
        } => {
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

/// Walk the AST of `file` collecting every reference to `name` and
/// return each as an LSP `Location` (URI + range) inside `uri`. When
/// `include_declaration` is true, definition sites (assignment
/// targets, loop variables, named function definitions) are included
/// alongside the plain identifier references; when false, only genuine
/// reference sites are returned.
///
/// Each returned `Location`'s range is derived from the matching
/// node's `Span` byte offsets against `text` so editors highlight
/// exactly the identifier characters. Zero-width spans are widened by
/// one character so the highlight is always visible.
fn find_references_in_file(
    file: &SourceFile,
    name: &str,
    uri: &Url,
    text: &str,
    include_declaration: bool,
) -> Vec<Location> {
    let mut spans: Vec<Span> = Vec::new();
    for stmt in &file.stmts {
        find_ref_spans_in_stmt(stmt, name, &mut spans, include_declaration);
    }
    let mut locations = Vec::with_capacity(spans.len());
    for span in spans {
        let start = byte_offset_to_position(text, span.start);
        let end = byte_offset_to_position(text, span.end);
        // Extend zero-width spans to one character so the editor
        // renders a non-empty highlight, mirroring
        // `diagnostic_to_lsp_with_source`'s behavior.
        let end = if start == end {
            Position {
                line: start.line,
                character: start.character + 1,
            }
        } else {
            end
        };
        locations.push(Location {
            uri: uri.clone(),
            range: Range { start, end },
        });
    }
    locations
}

/// Recurse into a statement collecting every reference to `name`,
/// appending each reference's `Span` to `out`. Definition sites
/// (assignment targets, loop variables, named function definitions)
/// are appended only when `include_declaration` is true.
///
/// The walk mirrors `find_def_spans_in_stmt` in structure: it recurses
/// into function bodies, `if`/`for`/`while` blocks, and the
/// sub-expressions of every statement so that references inside nested
/// scopes are found.
fn find_ref_spans_in_stmt(stmt: &Stmt, name: &str, out: &mut Vec<Span>, include_declaration: bool) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            // The assignment target (`x <- ...`) is a definition site.
            // Include it only when the caller asked for declarations.
            if include_declaration {
                if let Expr::Ident { name: n, span } = target {
                    if n == name {
                        out.push(*span);
                    }
                }
            }
            // The value always contributes references (e.g. `x <- x + 1`
            // references `x` on the right-hand side).
            find_ref_spans_in_expr(value, name, out, include_declaration);
        }
        Stmt::FunctionDef {
            name: fn_name,
            body,
            span,
            ..
        } => {
            // A named function definition is a declaration site. Include
            // it only when requested. (The parser currently always emits
            // `name: None`, but handle `Some` for completeness.)
            if include_declaration {
                if let Some(n) = fn_name {
                    if n == name {
                        out.push(*span);
                    }
                }
            }
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            find_ref_spans_in_expr(cond, name, out, include_declaration);
            for s in then {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
            if let Some(else_block) = else_ {
                for s in else_block {
                    find_ref_spans_in_stmt(s, name, out, include_declaration);
                }
            }
        }
        Stmt::For {
            name: loop_var,
            iter,
            body,
            span,
        } => {
            // The loop variable is a binding (definition site). Include
            // it only when the caller asked for declarations.
            if include_declaration && loop_var == name {
                out.push(*span);
            }
            // The iterator expression may reference `name`.
            find_ref_spans_in_expr(iter, name, out, include_declaration);
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Stmt::While { cond, body, .. } => {
            find_ref_spans_in_expr(cond, name, out, include_declaration);
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                find_ref_spans_in_expr(v, name, out, include_declaration);
            }
        }
        Stmt::Expr(e) => find_ref_spans_in_expr(e, name, out, include_declaration),
    }
}

/// Recurse into an expression collecting every reference to `name`,
/// appending each reference's `Span` to `out`. A `Expr::Ident` with a
/// matching name is the match target (a reference); all other variants
/// recurse into their sub-expressions so references inside calls,
/// operators, indexes, function literals, and conditional expressions
/// are found.
///
/// `include_declaration` is forwarded to `find_ref_spans_in_stmt` when
/// recursing into nested function bodies so that declaration inclusion
/// stays consistent across the whole AST.
fn find_ref_spans_in_expr(expr: &Expr, name: &str, out: &mut Vec<Span>, include_declaration: bool) {
    match expr {
        Expr::Ident { name: n, span } => {
            if n == name {
                out.push(*span);
            }
        }
        Expr::Call { func, args, .. } => {
            find_ref_spans_in_expr(func, name, out, include_declaration);
            for arg in args {
                find_ref_spans_in_expr(&arg.value, name, out, include_declaration);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            find_ref_spans_in_expr(lhs, name, out, include_declaration);
            find_ref_spans_in_expr(rhs, name, out, include_declaration);
        }
        Expr::UnaryOp { expr, .. } => find_ref_spans_in_expr(expr, name, out, include_declaration),
        Expr::Index { base, args, .. } => {
            find_ref_spans_in_expr(base, name, out, include_declaration);
            for arg in args {
                find_ref_spans_in_expr(&arg.value, name, out, include_declaration);
            }
        }
        Expr::Function { body, .. } => {
            for s in body {
                find_ref_spans_in_stmt(s, name, out, include_declaration);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            find_ref_spans_in_expr(cond, name, out, include_declaration);
            find_ref_spans_in_expr(then, name, out, include_declaration);
            if let Some(e) = else_ {
                find_ref_spans_in_expr(e, name, out, include_declaration);
            }
        }
        // Literals, NULL, NA, and Unknown carry no identifier
        // references.
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => {}
    }
}

/// Walk the AST of `file` collecting every occurrence of `name` in the
/// current file and classify each as a READ or WRITE highlight.
///
///   * WRITE: an occurrence that is the `target` of a `Stmt::Assign`
///     (i.e. the LHS of `name <- ...`). The loop variable of a
///     `for (name in ...)` is also treated as a WRITE since each
///     iteration re-binds it.
///   * READ: every other occurrence (e.g. on the RHS of an
///     assignment, in a call argument, in a condition).
///
/// Each returned `DocumentHighlight` carries the identifier's range
/// (derived from its `Span` byte offsets against `text`, with zero-width
/// spans widened by one character so the editor always renders a
/// visible highlight) and a `kind` of READ or WRITE.
fn collect_document_highlights(
    file: &SourceFile,
    name: &str,
    text: &str,
) -> Vec<DocumentHighlight> {
    let mut entries: Vec<(Span, DocumentHighlightKind)> = Vec::new();
    for stmt in &file.stmts {
        collect_highlight_entries_from_stmt(stmt, name, &mut entries);
    }
    entries
        .into_iter()
        .map(|(span, kind)| DocumentHighlight {
            range: span_to_visible_range(span, text),
            kind: Some(kind),
        })
        .collect()
}

/// Recurse into a statement collecting every occurrence of `name` as
/// a `(Span, DocumentHighlightKind)` pair. Assignment targets and loop
/// variables are WRITEs; every identifier reference is a READ. The walk
/// mirrors `find_ref_spans_in_stmt` so that occurrences inside nested
/// function bodies, `if`/`for`/`while` blocks, and operator / call /
/// index sub-expressions are all found.
fn collect_highlight_entries_from_stmt(
    stmt: &Stmt,
    name: &str,
    out: &mut Vec<(Span, DocumentHighlightKind)>,
) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            // The assignment target (`x <- ...`) is a WRITE. This also
            // covers `f <- function(..) ..` (the parser models named
            // function definitions as Assign with an Expr::Function
            // value).
            if let Expr::Ident { name: n, span } = target {
                if n == name {
                    out.push((*span, DocumentHighlightKind::WRITE));
                }
            }
            // The value contributes READ references (e.g. the RHS of
            // `x <- x + 1` reads `x`).
            collect_highlight_entries_from_expr(value, name, out);
        }
        Stmt::FunctionDef {
            name: fn_name,
            body,
            span,
            ..
        } => {
            // A named function definition is a WRITE binding. (The
            // parser currently always emits `name: None`, but handle
            // `Some` for completeness.)
            if let Some(n) = fn_name {
                if n == name {
                    out.push((*span, DocumentHighlightKind::WRITE));
                }
            }
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            collect_highlight_entries_from_expr(cond, name, out);
            for s in then {
                collect_highlight_entries_from_stmt(s, name, out);
            }
            if let Some(else_block) = else_ {
                for s in else_block {
                    collect_highlight_entries_from_stmt(s, name, out);
                }
            }
        }
        Stmt::For {
            name: loop_var,
            iter,
            body,
            span,
        } => {
            // The loop variable is re-bound each iteration, so it is a
            // WRITE. This matches how the task spec defines WRITE
            // ("assignment target"), and how the editor visually
            // distinguishes definition vs usage.
            if loop_var == name {
                out.push((*span, DocumentHighlightKind::WRITE));
            }
            // The iterator expression may READ `name`.
            collect_highlight_entries_from_expr(iter, name, out);
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_highlight_entries_from_expr(cond, name, out);
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                collect_highlight_entries_from_expr(v, name, out);
            }
        }
        Stmt::Expr(e) => collect_highlight_entries_from_expr(e, name, out),
    }
}

/// Recurse into an expression collecting every READ occurrence of
/// `name` as a `(Span, DocumentHighlightKind::READ)` pair. A
/// `Expr::Ident` with a matching name is the match target (a READ);
/// all other variants recurse into their sub-expressions so reads
/// inside calls, operators, indexes, function literals, and
/// conditional expressions are found.
fn collect_highlight_entries_from_expr(
    expr: &Expr,
    name: &str,
    out: &mut Vec<(Span, DocumentHighlightKind)>,
) {
    match expr {
        Expr::Ident { name: n, span } => {
            if n == name {
                out.push((*span, DocumentHighlightKind::READ));
            }
        }
        Expr::Call { func, args, .. } => {
            collect_highlight_entries_from_expr(func, name, out);
            for arg in args {
                collect_highlight_entries_from_expr(&arg.value, name, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_highlight_entries_from_expr(lhs, name, out);
            collect_highlight_entries_from_expr(rhs, name, out);
        }
        Expr::UnaryOp { expr, .. } => collect_highlight_entries_from_expr(expr, name, out),
        Expr::Index { base, args, .. } => {
            collect_highlight_entries_from_expr(base, name, out);
            for arg in args {
                collect_highlight_entries_from_expr(&arg.value, name, out);
            }
        }
        Expr::Function { body, .. } => {
            for s in body {
                collect_highlight_entries_from_stmt(s, name, out);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            collect_highlight_entries_from_expr(cond, name, out);
            collect_highlight_entries_from_expr(then, name, out);
            if let Some(e) = else_ {
                collect_highlight_entries_from_expr(e, name, out);
            }
        }
        // Literals, NULL, NA, and Unknown carry no identifier
        // references.
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => {}
    }
}

/// Convert a `Span`'s byte offsets to an LSP `Range` against `text`.
/// Zero-width spans are widened by one character so the editor always
/// renders a non-empty highlight, mirroring the convention used by
/// `find_references_in_file` and `diagnostic_to_lsp_with_source`.
fn span_to_visible_range(span: Span, text: &str) -> Range {
    let start = byte_offset_to_position(text, span.start);
    let end = byte_offset_to_position(text, span.end);
    let end = if start == end {
        Position {
            line: start.line,
            character: start.character + 1,
        }
    } else {
        end
    };
    Range { start, end }
}

/// Convert a document's path string (the key used in `State::docs`)
/// back into an LSP `Url`. The common case is a filesystem path
/// produced by `uri_to_path`, which round-trips cleanly through
/// `Url::from_file_path`. Non-file documents (e.g. `untitled:` URIs
/// that fell back to their string form in `uri_to_path`) are recovered
/// via `Url::parse`.
fn path_to_uri(path: &str) -> Url {
    Url::from_file_path(path).unwrap_or_else(|_| {
        Url::parse(path).unwrap_or_else(|_| Url::parse("file:///unknown").unwrap())
    })
}

/// Build a `WorkspaceEdit` renaming `old_name` to `new_name` across
/// the given slice of `(path, parsed_file, source_text)` tuples.
/// Mirrors exactly what the `rename` LSP method does, minus the async
/// state lookup, so the rename logic is unit-testable without a live
/// `Backend`.
///
/// For each document we reuse `find_references_in_file` (with
/// `include_declaration = true`, since a rename must rewrite the
/// definition site as well as every read / call site) and append one
/// `TextEdit` per occurrence. Edits are grouped by file URI into the
/// `WorkspaceEdit.changes` map.
#[cfg(test)]
fn build_rename_edits(
    docs: &[(&str, &SourceFile, &str)],
    old_name: &str,
    new_name: &str,
) -> WorkspaceEdit {
    let mut edits: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for (doc_path, file, doc_text) in docs {
        let doc_uri = path_to_uri(doc_path);
        let locations = find_references_in_file(file, old_name, &doc_uri, doc_text, true);
        for loc in locations {
            edits.entry(doc_uri.clone()).or_default().push(TextEdit {
                range: loc.range,
                new_text: new_name.to_string(),
            });
        }
    }
    WorkspaceEdit {
        changes: Some(edits),
        ..Default::default()
    }
}

/// Collect `InlayHint`s for every assignment whose target is a bare
/// identifier with a known (non-opaque) inferred type. The hint is
/// placed at the end of the identifier name (so the editor renders the
/// ghost text right after the variable, before the `<-`), and its
/// label is the inferred type rendered via `RType`'s `Display` impl
/// (e.g. `: integer<len=1>`).
///
/// The walk recurses into `Stmt::FunctionDef` bodies so that local
/// bindings inside named functions are annotated too (the top-level
/// scope may or may not track them; if it doesn't, the lookup simply
/// yields `None` and no hint is emitted, which is the right call for
/// v1).
///
/// Opaque (`Mode::Opaque`) types are deliberately skipped: they
/// represent "we don't know" and would only clutter the editor with
/// unhelpful `: opaque<len=?>?NA?` annotations. This mirrors how the
/// `document_symbol` detail path behaves implicitly (it surfaces
/// whatever the scope has, but for opaque the Display string is
/// noisy). For inlay hints, skipping is the better UX.
fn collect_inlay_hints(file: &SourceFile, scope: &Scope, text: &str) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    for stmt in &file.stmts {
        collect_inlay_hints_from_stmt(stmt, scope, text, &mut hints);
    }
    hints
}

/// Walk a single statement, appending any inlay hints it contributes
/// to `hints`. Assignments to a bare identifier become hints (when
/// the scope has a non-opaque type for the name); function-definition
/// statements are recursed into so their body bindings are annotated.
fn collect_inlay_hints_from_stmt(
    stmt: &Stmt,
    scope: &Scope,
    text: &str,
    hints: &mut Vec<InlayHint>,
) {
    match stmt {
        // Destructure the target directly so clippy's `collapsible_match`
        // lint stays quiet: only bare-identifier targets become hints.
        // Complex targets (`df$col <- 1`, `x[1] <- 2`) fall through to
        // the second `Stmt::Assign` arm below and contribute nothing.
        Stmt::Assign {
            target: Expr::Ident { name, span },
            ..
        } => {
            if let Some(t) = scope.get(name) {
                // Skip opaque types: they're not useful to the
                // user (they represent "we don't know"). Showing
                // `: opaque<len=?>?NA?` next to every unknown
                // binding would just be visual noise.
                if matches!(t.mode, ry_core::types::Mode::Opaque) {
                    return;
                }
                // Place the hint right after the identifier name.
                // `span.start + name.len()` lands on the first
                // character past the identifier (in byte space,
                // which `byte_offset_to_position` converts to an
                // LSP `Position`). For ASCII identifiers this is
                // exact; non-ASCII names would need a UTF-16-aware
                // helper, matching the existing approximation in
                // `byte_offset_to_position`.
                let pos = byte_offset_to_position(text, span.start + name.len());
                hints.push(InlayHint {
                    position: pos,
                    label: InlayHintLabel::String(format!(": {}", t)),
                    kind: Some(InlayHintKind::TYPE),
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    text_edits: None,
                    data: None,
                });
            }
        }
        // Non-identifier assignment targets (e.g. `x[1] <- 2`,
        // `df$col <- value`) don't introduce a new name in the
        // scope, so they contribute no hints.
        Stmt::Assign { .. } => {}
        // Recurse into named function bodies so nested bindings are
        // annotated too. `Stmt::FunctionDef` with `name: Some(..)` is
        // not currently emitted by the parser (named functions come
        // through as `Assign` + `Expr::Function`), but we handle it
        // for completeness / future grammar changes.
        Stmt::FunctionDef { body, .. } => {
            for s in body {
                collect_inlay_hints_from_stmt(s, scope, text, hints);
            }
        }
        // Other statement forms (bare expressions, control flow,
        // returns) do not introduce named top-level bindings, so they
        // contribute no hints. We deliberately do NOT recurse into
        // `if`/`for`/`while` bodies here (unlike `collect_symbols`)
        // because the top-level scope only tracks the file's top
        // scope; bindings introduced inside control-flow blocks may
        // not be present in `scope`, and emitting a hint for a name
        // the scope doesn't know would be wrong.
        _ => {}
    }
}

/// Collect completion items for a given cursor position and trigger
/// context. The decision tree mirrors what R users expect from an
/// autocomplete popup:
///
///   * If the user just typed `$` after an identifier whose scope
///     type carries a `ColumnSchema` (e.g. a `list(a = 1, b = 2)`
///     literal or a `data.frame(...)` call), return ONLY the column
///     names as `FIELD` items. Dumping the rest of the scope here
///     would be noise: the user is clearly asking for a column.
///   * Otherwise (manual invocation, identifier character, `:`, etc.)
///     return the in-scope bindings (`VARIABLE` / `FUNCTION`) plus a
///     curated list of common base-R keywords and functions. This
///     gives a focused, predictable popup instead of a giant dump.
///
/// The list is sorted alphabetically by `label` and de-duplicated so
/// the same name never appears twice (e.g. when a user-defined `c`
/// would otherwise collide with the curated `c`).
fn collect_completions(
    text: &str,
    position: Position,
    context: &Option<CompletionContext>,
    scope: &Scope,
) -> Vec<CompletionItem> {
    let trigger = context
        .as_ref()
        .and_then(|c| c.trigger_character.as_deref());

    if trigger == Some("$") {
        // `$`-triggered completion: offer only column names from the
        // variable before the `$` on the current line. If the
        // variable is unknown or carries no schema, return an empty
        // list (no completions) rather than falling through to the
        // generic list, so the editor popup stays focused.
        if let Some(line) = text.lines().nth(position.line as usize) {
            // `position.character` is a UTF-16 offset; we
            // approximate it as a byte index (matching the rest of
            // this file's ASCII assumption). Clamp to the line end
            // so a cursor past the last char doesn't slice out of
            // bounds.
            let until = position.character.min(line.len() as u32) as usize;
            let before_cursor = &line[..until];
            // Strip the trailing `$` (and any whitespace between the
            // identifier and it) so `extract_last_identifier` lands
            // on the variable name.
            let trimmed = before_cursor.trim_end().trim_end_matches('$');
            if let Some(var_name) = extract_last_identifier(trimmed) {
                if let Some(t) = scope.get(&var_name) {
                    if let Some(schema) = &t.columns {
                        let mut items: Vec<CompletionItem> = schema
                            .columns
                            .iter()
                            .map(|(col_name, col_type)| CompletionItem {
                                label: col_name.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(format!("{}", col_type)),
                                ..Default::default()
                            })
                            .collect();
                        items.sort_by(|a, b| a.label.cmp(&b.label));
                        return items;
                    }
                }
            }
        }
        // No schema (or no variable) before the `$`: nothing useful
        // to offer. Returning empty lets the editor close the popup
        // instead of showing irrelevant completions.
        return Vec::new();
    }

    // Generic completion: variables in scope + common keywords /
    // functions. We surface every checked binding (so locally defined
    // variables and functions complete) and then layer in the small
    // curated list of base-R names. The curated list is intentionally
    // short: the task explicitly calls for a focused popup.
    let mut items: Vec<CompletionItem> = scope
        .bindings
        .iter()
        .map(|(name, t)| CompletionItem {
            label: name.clone(),
            kind: Some(if matches!(t.mode, Mode::Function) {
                CompletionItemKind::FUNCTION
            } else {
                CompletionItemKind::VARIABLE
            }),
            detail: Some(format!("{}", t)),
            ..Default::default()
        })
        .collect();
    items.extend(common_r_completions());

    // Sort alphabetically by label, then drop duplicates (a user
    // `c <- ...` binding would otherwise collide with the curated
    // `c`). `dedup_by` after `sort_by` collapses only adjacent
    // equal-label pairs, so the sort must use the same key.
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items.dedup_by(|a, b| a.label == b.label);
    items
}

/// Build a small, curated list of common base-R keywords and
/// functions. Kept short on purpose: the task calls for a focused
/// popup, and the typeshed's full function table isn't directly
/// reachable from the LSP crate. These names cover the constructs R
/// users type most often at the top level.
fn common_r_completions() -> Vec<CompletionItem> {
    // (name, kind, detail). The detail is a one-line human hint so
    // the popup shows something useful next to each entry. We use the
    // full `CompletionItemKind::X` form (rather than a `use` alias)
    // because `CompletionItemKind` is a tuple struct with associated
    // constants, not an enum, so a glob import is not allowed.
    const ENTRIES: &[(&str, CompletionItemKind, &str)] = &[
        // Keywords / control flow.
        ("if", CompletionItemKind::KEYWORD, "conditional"),
        (
            "else",
            CompletionItemKind::KEYWORD,
            "conditional alternative",
        ),
        ("for", CompletionItemKind::KEYWORD, "for loop"),
        ("while", CompletionItemKind::KEYWORD, "while loop"),
        ("repeat", CompletionItemKind::KEYWORD, "repeat loop"),
        (
            "function",
            CompletionItemKind::KEYWORD,
            "function definition",
        ),
        (
            "return",
            CompletionItemKind::KEYWORD,
            "return from function",
        ),
        ("break", CompletionItemKind::KEYWORD, "break out of loop"),
        (
            "next",
            CompletionItemKind::KEYWORD,
            "skip to next iteration",
        ),
        // Common base-R functions.
        (
            "c",
            CompletionItemKind::FUNCTION,
            "combine values into a vector",
        ),
        ("list", CompletionItemKind::FUNCTION, "create a list"),
        (
            "data.frame",
            CompletionItemKind::FUNCTION,
            "create a data frame",
        ),
        ("matrix", CompletionItemKind::FUNCTION, "create a matrix"),
        ("vector", CompletionItemKind::FUNCTION, "create a vector"),
        (
            "length",
            CompletionItemKind::FUNCTION,
            "length of an object",
        ),
        ("names", CompletionItemKind::FUNCTION, "names of an object"),
        ("mean", CompletionItemKind::FUNCTION, "arithmetic mean"),
        ("sum", CompletionItemKind::FUNCTION, "sum of elements"),
        ("min", CompletionItemKind::FUNCTION, "minimum"),
        ("max", CompletionItemKind::FUNCTION, "maximum"),
        ("print", CompletionItemKind::FUNCTION, "print an object"),
        (
            "str",
            CompletionItemKind::FUNCTION,
            "display the structure of an object",
        ),
        (
            "library",
            CompletionItemKind::FUNCTION,
            "load an attached package",
        ),
        (
            "require",
            CompletionItemKind::FUNCTION,
            "load an attached package",
        ),
        (
            "sapply",
            CompletionItemKind::FUNCTION,
            "apply a function over a list or vector",
        ),
        (
            "lapply",
            CompletionItemKind::FUNCTION,
            "apply a function over a list",
        ),
        (
            "mapply",
            CompletionItemKind::FUNCTION,
            "apply a function over multiple arguments",
        ),
        (
            "which",
            CompletionItemKind::FUNCTION,
            "indices of TRUE values",
        ),
        (
            "is.na",
            CompletionItemKind::FUNCTION,
            "detect missing values",
        ),
        (
            "as.integer",
            CompletionItemKind::FUNCTION,
            "coerce to integer",
        ),
        (
            "as.numeric",
            CompletionItemKind::FUNCTION,
            "coerce to numeric",
        ),
        (
            "as.character",
            CompletionItemKind::FUNCTION,
            "coerce to character",
        ),
        (
            "as.logical",
            CompletionItemKind::FUNCTION,
            "coerce to logical",
        ),
    ];
    ENTRIES
        .iter()
        .map(|(name, kind, detail)| CompletionItem {
            label: (*name).to_string(),
            kind: Some(*kind),
            detail: Some((*detail).to_string()),
            ..Default::default()
        })
        .collect()
}

/// Extract the identifier at the end of `s`, scanning backwards. An
/// "identifier character" follows R's rules: ASCII alphanumeric, `_`,
/// or `.`. Returns `None` when `s` does not end with an identifier
/// character (e.g. `s == ""`, `s == "()"`, or `s == "$"`).
///
/// This is used by `$`-triggered completion to recover the variable
/// name preceding the `$` (e.g. `mtcars$` -> `mtcars`). It is a
/// simple character scan, not a parser query, which is enough for
/// the common single-line case `var$`.
fn extract_last_identifier(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut end = chars.len();
    while end > 0
        && (chars[end - 1].is_alphanumeric() || chars[end - 1] == '_' || chars[end - 1] == '.')
    {
        end -= 1;
    }
    if end < chars.len() {
        Some(chars[end..].iter().collect())
    } else {
        None
    }
}

/// Find the enclosing function call for a cursor at `(line, col)` in
/// `text`. Returns `(function_name, active_param_index)` where
/// `function_name` is the identifier immediately before the nearest
/// unmatched `(` to the left of the cursor, and `active_param_index`
/// is the number of commas at depth 0 between the `(` and the cursor.
///
/// The scan is confined to the current line (matching the common case
/// where the user is mid-call on a single line). Returns `None` when:
///   * the line doesn't exist;
///   * there is no unmatched `(` before the cursor (cursor not in a
///     call);
///   * the text immediately before the `(` is not an identifier
///     (e.g. the cursor sits inside `1 + (2 *` rather than a function
///     call).
fn find_enclosing_call(text: &str, line: usize, col: usize) -> Option<(String, usize)> {
    let line_str = text.lines().nth(line)?;
    // Clamp the column to the line length so a cursor past the last
    // character (a common transient state right after typing `(`)
    // doesn't slice out of bounds. `col` is treated as a byte index,
    // matching the ASCII assumption used throughout this file.
    let until = col.min(line_str.len());
    let before_cursor = &line_str[..until];

    // Walk backward to find the last unmatched `(`. We track depth so
    // a `(` belonging to a nested call (e.g. the inner `(` in
    // `f(g(`) is skipped in favor of the outermost enclosing one.
    let mut depth = 0;
    let mut paren_pos = None;
    for (i, ch) in before_cursor.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    paren_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let paren_pos = paren_pos?;

    // The function name is the identifier ending right at the `(`.
    // `extract_last_identifier` already scans backward for an R-style
    // identifier, which is exactly what we need.
    let before_paren = &before_cursor[..paren_pos];
    let func_name = extract_last_identifier(before_paren)?;

    // Count commas between `(` and the cursor to determine which
    // parameter the user is currently editing. We only count commas at
    // depth 0 (commas inside nested calls belong to the inner call's
    // argument list, not this one). Strings are not tracked here, so
    // a comma inside a string literal would be miscounted; that's an
    // acceptable v1 approximation for the common case.
    let args_str = &before_cursor[paren_pos + 1..];
    let mut local_depth = 0;
    let mut active_param = 0;
    for ch in args_str.chars() {
        match ch {
            '(' | '[' | '{' => local_depth += 1,
            ')' | ']' | '}' => {
                if local_depth > 0 {
                    local_depth -= 1;
                }
            }
            ',' if local_depth == 0 => active_param += 1,
            _ => {}
        }
    }

    Some((func_name, active_param))
}

/// Look up the formal parameter names of a base-R function for
/// signature help. Returns `None` for functions outside the curated
/// table (user-defined functions are out of scope; the checker's
/// FnTable isn't reachable from the LSP crate).
///
/// The table is a small hand-maintained list of the most common base-R
/// functions with their conventional parameter names. `...` is used
/// for variadic functions where naming the rest of the parameters
/// would be misleading. This intentionally avoids the typeshed: it
/// would require exposing `ry-typeshed`'s internal `params` arrays to
/// the LSP crate, and the curated list covers the cases users hit most.
fn get_signature(name: &str) -> Option<Vec<String>> {
    let params: &[&str] = match name {
        "c" => &["..."],
        "list" => &["..."],
        "mean" => &["x", "trim", "na.rm"],
        "sum" => &["..."],
        "length" => &["x"],
        "rep" => &["x", "times", "each"],
        "seq" => &["from", "to", "by"],
        "round" => &["x", "digits"],
        "paste" => &["...", "sep", "collapse"],
        "paste0" => &["...", "collapse"],
        "sprintf" => &["fmt", "..."],
        "lapply" => &["X", "FUN"],
        "sapply" => &["X", "FUN"],
        "vapply" => &["X", "FUN", "FUN.VALUE"],
        "mapply" => &["FUN", "..."],
        "Map" => &["f", "..."],
        "Reduce" => &["f", "x", "accumulate"],
        "grepl" => &["pattern", "x"],
        "gsub" => &["pattern", "replacement", "x"],
        "substr" => &["x", "start", "stop"],
        "matrix" => &["data", "nrow", "ncol"],
        "data.frame" => &["..."],
        "factor" => &["x", "levels", "labels"],
        "ifelse" => &["test", "yes", "no"],
        "which" => &["x"],
        "order" => &["..."],
        "sort" => &["x"],
        "unique" => &["x"],
        "match" => &["x", "table"],
        "names" => &["x"],
        "nchar" => &["x"],
        "toupper" => &["x"],
        "tolower" => &["x"],
        "print" => &["x"],
        "cat" => &["..."],
        "stop" => &["..."],
        "warning" => &["..."],
        "nrow" => &["x"],
        "ncol" => &["x"],
        "head" => &["x", "n"],
        "tail" => &["x", "n"],
        "cbind" => &["..."],
        "rbind" => &["..."],
        "merge" => &["x", "y"],
        _ => return None,
    };
    Some(params.iter().map(|s| s.to_string()).collect())
}

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
fn flatten_symbols_to_symbol_info(
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
    Position {
        line,
        character: col,
    }
}

/// Collect `FoldingRange`s for every multi-line foldable block in the
/// file. R's foldable regions are:
///   * function bodies (`function() { ... }`),
///   * `if`/`else` blocks,
///   * `for`/`while` loop bodies,
///   * assignment statements whose RHS spans multiple lines (e.g.
///     `f <- function() { ... }`, since the parser models named
///     function definitions as `Assign` rather than `FunctionDef`).
///
/// A region is foldable only when its `Span` crosses a newline
/// (single-line blocks have nothing to collapse). The walk recurses
/// into nested bodies so an outer function and an inner `if` each get
/// their own range.
fn collect_folding_ranges(file: &SourceFile, text: &str) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    for stmt in &file.stmts {
        collect_folding_from_stmt(stmt, text, &mut ranges);
    }
    ranges
}

/// Walk a single statement, appending any foldable region it
/// contributes to `ranges`, and then recurse into nested blocks so
/// their inner regions are also collected.
fn collect_folding_from_stmt(stmt: &Stmt, text: &str, ranges: &mut Vec<FoldingRange>) {
    // Compute this statement's span and, if it spans multiple lines,
    // push a folding range for it. We compute the end line from the
    // span's byte offsets against the source text (so multi-line
    // bodies that share their starting line with the `function` /
    // `for` / `if` keyword are folded correctly).
    if let Some(span) = span_of_stmt(stmt) {
        let start_line = span.line as u32;
        let end_line = byte_offset_to_position(text, span.end).line;
        if end_line > start_line {
            ranges.push(FoldingRange {
                start_line,
                end_line,
                start_character: None,
                end_character: None,
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            });
        }
    }
    // Recurse into nested blocks so an outer function's body and an
    // inner `if` each get their own range. Mirrors
    // `collect_from_stmt`'s structure (used by `documentSymbol`).
    match stmt {
        Stmt::FunctionDef { body, .. } | Stmt::For { body, .. } | Stmt::While { body, .. } => {
            for s in body {
                collect_folding_from_stmt(s, text, ranges);
            }
        }
        Stmt::If { then, else_, .. } => {
            for s in then {
                collect_folding_from_stmt(s, text, ranges);
            }
            if let Some(e) = else_ {
                for s in e {
                    collect_folding_from_stmt(s, text, ranges);
                }
            }
        }
        // An `Assign` may carry a multi-line function literal on its
        // RHS (the common `f <- function() { ... }` pattern). Recurse
        // into the value so nested function bodies and control-flow
        // blocks inside it also become foldable. `Expr::Function` and
        // `Expr::If` both expose their own spans, so the recursive
        // `collect_folding_from_expr` can pick them up.
        Stmt::Assign { value, .. } => {
            collect_folding_from_expr(value, text, ranges);
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                collect_folding_from_expr(v, text, ranges);
            }
        }
        Stmt::Expr(e) => collect_folding_from_expr(e, text, ranges),
    }
}

/// Recurse into an expression looking for nested multi-line blocks
/// (function literals, conditional expressions, and the bodies of
/// call / operator / index sub-expressions) so they contribute their
/// own folding ranges.
fn collect_folding_from_expr(expr: &Expr, text: &str, ranges: &mut Vec<FoldingRange>) {
    match expr {
        Expr::Function { body, span, .. } => {
            // The function literal itself is a foldable region; its
            // body statements may contain more foldable regions.
            push_range_if_multiline(*span, text, ranges);
            for s in body {
                collect_folding_from_stmt(s, text, ranges);
            }
        }
        Expr::If {
            cond,
            then,
            else_,
            span,
        } => {
            push_range_if_multiline(*span, text, ranges);
            collect_folding_from_expr(cond, text, ranges);
            collect_folding_from_expr(then, text, ranges);
            if let Some(e) = else_ {
                collect_folding_from_expr(e, text, ranges);
            }
        }
        Expr::Call {
            func, args, span, ..
        } => {
            push_range_if_multiline(*span, text, ranges);
            collect_folding_from_expr(func, text, ranges);
            for arg in args {
                collect_folding_from_expr(&arg.value, text, ranges);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_folding_from_expr(lhs, text, ranges);
            collect_folding_from_expr(rhs, text, ranges);
        }
        Expr::UnaryOp { expr, .. } => collect_folding_from_expr(expr, text, ranges),
        Expr::Index { base, args, .. } => {
            collect_folding_from_expr(base, text, ranges);
            for arg in args {
                collect_folding_from_expr(&arg.value, text, ranges);
            }
        }
        // Literals, NULL, NA, bare identifiers, and Unknown carry no
        // nested foldable bodies.
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

/// Push a folding range for `span` when its end lands on a later line
/// than its start. Mirrors the inline check used in
/// `collect_folding_from_stmt` so the same "multi-line" rule is
/// applied uniformly to statement and expression spans.
fn push_range_if_multiline(span: Span, text: &str, ranges: &mut Vec<FoldingRange>) {
    let start_line = span.line as u32;
    let end_line = byte_offset_to_position(text, span.end).line;
    if end_line > start_line {
        ranges.push(FoldingRange {
            start_line,
            end_line,
            start_character: None,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        });
    }
}

/// Return the `Span` of a statement, if it carries one. Most
/// statement variants store their own span; the bare-expression form
/// (`Stmt::Expr`) delegates to the inner expression's span (when one
/// is available).
fn span_of_stmt(stmt: &Stmt) -> Option<Span> {
    match stmt {
        Stmt::Assign { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::FunctionDef { span, .. }
        | Stmt::Return { span, .. } => Some(*span),
        Stmt::Expr(e) => span_of_expr(e),
    }
}

/// Return the `Span` of an expression. Each variant that wraps its
/// own span contributes it directly; bare literals return their span
/// too. Returns `None` only for variants that have no span at all
/// (none in the current AST, but the future-proofing keeps the
/// compiler honest).
fn span_of_expr(expr: &Expr) -> Option<Span> {
    match expr {
        Expr::Logical(_, s)
        | Expr::Integer(_, s)
        | Expr::Double(_, s)
        | Expr::String(_, s)
        | Expr::Null(s)
        | Expr::Na(_, s)
        | Expr::Call { span: s, .. }
        | Expr::Ident { span: s, .. }
        | Expr::BinOp { span: s, .. }
        | Expr::UnaryOp { span: s, .. }
        | Expr::Index { span: s, .. }
        | Expr::Function { span: s, .. }
        | Expr::If { span: s, .. }
        | Expr::Unknown(s) => Some(*s),
    }
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

/// Extract the diagnostic code string from an LSP `Diagnostic`. ry
/// always emits string codes (`RY040`, `RY001`, ...), but we handle
/// the numeric variant defensively so a future change to the code
/// shape does not crash the code-action handler. Returns an empty
/// string when the diagnostic has no code, in which case the ignore
/// comment omits the `[CODE]` suffix.
fn diag_code_from_lsp(d: &LspDiagnostic) -> String {
    match &d.code {
        Some(NumberOrString::String(s)) => s.clone(),
        Some(NumberOrString::Number(n)) => n.to_string(),
        None => String::new(),
    }
}

/// Build a `CodeAction` that appends a `# ry: ignore[CODE]`
/// suppression comment to the end of the diagnostic's line. Returns
/// `None` when the line already carries an ignore comment (so the
/// lightbulb does not offer a redundant no-op).
///
/// The produced `TextEdit` replaces the whole line with
/// `<original line>  # ry: ignore[<code>]` (two spaces before the `#`
/// to visually separate the code from the comment). The `CodeAction`
/// also carries the originating diagnostic in `diagnostics` so the
/// editor can link the lightbulb back to the squiggle it fixes.
fn make_ignore_action(uri: &Url, diag: &LspDiagnostic, text: &str) -> Option<CodeAction> {
    let line = diag.range.start.line as usize;
    let line_text = text.lines().nth(line)?;

    // A line that already carries an ignore comment is fully
    // suppressed; offering another action would be a no-op.
    if line_text.contains("ry: ignore") {
        return None;
    }

    let code = diag_code_from_lsp(diag);
    let new_line = if code.is_empty() {
        format!("{}  # ry: ignore", line_text)
    } else {
        format!("{}  # ry: ignore[{}]", line_text, code)
    };

    let start = Position {
        line: diag.range.start.line,
        character: 0,
    };
    let end = Position {
        line: diag.range.start.line,
        character: line_text.len() as u32,
    };

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range { start, end },
            new_text: new_line,
        }],
    );

    let title = if code.is_empty() {
        "Ignore this diagnostic on its line".to_string()
    } else {
        format!("Ignore {} on this line", code)
    };

    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        diagnostics: Some(vec![diag.clone()]),
        ..Default::default()
    })
}

/// Build a `CodeAction` that inserts `# ry: ignore-file` at the top
/// of the document (line 0, character 0), suppressing every ry
/// diagnostic in the file. Returns `None` when the file already
/// carries a file-level suppression comment so the lightbulb does
/// not offer a redundant no-op.
fn make_ignore_file_action(uri: &Url, text: &str) -> Option<CodeAction> {
    if text.contains("ry: ignore-file") {
        return None;
    }

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            new_text: "# ry: ignore-file\n".to_string(),
        }],
    );

    Some(CodeAction {
        title: "Ignore all diagnostics in this file".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Map an LSP `Position` (0-indexed line, 0-indexed character column)
/// to a byte offset within `text`. Mirrors `byte_offset_to_position`'s
/// ASCII assumption: each `char` advances the column by one, each
/// newline bumps the line. For non-ASCII content this is an
/// approximation consistent with the rest of the file.
///
/// Used by `find_enclosing_stmt_range` to test whether a cursor
/// position falls within a statement's byte-offset `Span`.
fn position_to_byte_offset(text: &str, position: Position) -> usize {
    let mut line = 0u32;
    let mut col = 0u32;
    for (b, ch) in text.char_indices() {
        if line == position.line && col >= position.character {
            return b;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    text.len()
}

/// Find the smallest enclosing statement whose `Span` contains the
/// cursor position, returning its range as an LSP `Range`. Returns
/// `None` when the cursor is not inside any statement (e.g. on a
/// blank line past the end of the file).
///
/// The byte offset of the cursor is computed via
/// `position_to_byte_offset`; we then walk the file's top-level
/// statements and return the first whose span contains that offset.
/// Nested statements (function bodies, control-flow blocks) are not
/// searched: for the expand-selection use case, expanding from the
/// identifier to the top-level statement is the most useful single
/// hop, and the file-level range provides the "expand all the way"
/// step.
fn find_enclosing_stmt_range(position: Position, file: &SourceFile, text: &str) -> Option<Range> {
    let byte_offset = position_to_byte_offset(text, position);
    let mut best: Option<Span> = None;
    for stmt in &file.stmts {
        if let Some(span) = span_of_stmt(stmt) {
            if byte_offset >= span.start && byte_offset < span.end {
                // Prefer the smallest (innermost) enclosing statement
                // so multi-statement lines and nested constructs
                // expand to the tightest meaningful range first.
                match best {
                    None => best = Some(span),
                    Some(prev) if span.end - span.start < prev.end - prev.start => {
                        best = Some(span);
                    }
                    _ => {}
                }
            }
        }
    }
    best.and_then(|span| span_to_range(text, span))
}

/// Compute an LSP `Range` covering the entire source text (from
/// `(0, 0)` to the position of the last byte). Used as the widest
/// level of the selection-range chain.
fn file_range(text: &str) -> Range {
    let end = byte_offset_to_position(text, text.len());
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end,
    }
}

/// Build a `SelectionRange` chain for a single cursor position. The
/// chain widens from the identifier under the cursor (narrowest) to
/// the enclosing statement (middle) to the whole file (widest),
/// matching how VS Code's "Expand Selection" works.
///
/// When the cursor is not on an identifier (e.g. on whitespace or an
/// operator), the narrowest range falls back to a zero-width span at
/// the cursor so the editor still has something to anchor the
/// selection. Levels that would be identical to their child (e.g. a
/// single-statement file where the statement span equals the file
/// span) are skipped so the chain never contains duplicate ranges.
fn build_selection_range(position: Position, file: &SourceFile, text: &str) -> SelectionRange {
    // 1. The identifier at the cursor (narrowest). Fall back to a
    //    zero-width span at the cursor when the position is not on an
    //    identifier-like character.
    let ident_range = find_identifier_range_at_position(
        text,
        position.line as usize,
        position.character as usize,
    )
    .map(|(_, r)| r)
    .unwrap_or(Range {
        start: position,
        end: position,
    });

    // 2. The enclosing statement (middle).
    let stmt_range = find_enclosing_stmt_range(position, file, text);

    // 3. The whole file (widest).
    let file_range = file_range(text);

    // Build the chain from widest to narrowest so each level's
    // `parent` points to the next wider range. Duplicate levels are
    // skipped so the editor never offers a no-op expand step.
    //
    // Per the LSP spec, the outermost `SelectionRange` is the
    // narrowest; each `parent` widens. We therefore start from the
    // widest range (file) as the deepest parent and wrap outward.
    let mut chain: Vec<Range> = vec![file_range];
    if let Some(stmt) = stmt_range {
        if stmt != file_range && stmt != ident_range {
            chain.push(stmt);
        }
    }
    if ident_range != *chain.last().unwrap() {
        chain.push(ident_range);
    }

    // Fold the chain into nested `SelectionRange`s. The chain is
    // ordered widest-first; we build from the widest (deepest parent)
    // and wrap each narrower level around it so the outermost
    // `SelectionRange.range` is the narrowest (identifier).
    let mut sel = SelectionRange {
        range: chain[0],
        parent: None,
    };
    for r in chain.into_iter().skip(1) {
        sel = SelectionRange {
            range: r,
            parent: Some(Box::new(sel)),
        };
    }
    sel
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
    Server::new(stdin, stdout, socket).serve(service).await;
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
        let src =
            "add <- function(x = 0, y = 0) { x + y }\nresult <- add(1, 2)\nname <- \"hello\"\n";
        let symbols = doc_symbols(src);
        assert_eq!(symbols.len(), 3, "got {:?}", symbols);

        assert_eq!(symbols[0].name, "add");
        assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
        // The checker infers a function type for `add`, so the detail
        // surfaces that (which starts with "function"). We don't pin
        // the exact signature since return-type inference may refine
        // it over time; we just check it identifies a function.
        let detail = symbols[0]
            .detail
            .as_deref()
            .expect("add should have detail");
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

    // ---- inlay hint helpers ----

    /// Helper: parse + check a snippet and return its inlay hints.
    /// Mirrors what the `inlay_hint` LSP method does, minus the async
    /// state lookup and range filter.
    fn inlay_hints(src: &str) -> Vec<InlayHint> {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let mut checker = ry_checker::Checker::new("test.R");
        let (_, scope) = checker.check_with_scope(&file);
        collect_inlay_hints(&file, &scope, src)
    }

    #[test]
    fn inlay_hints_for_basic_assignments() {
        // The canonical example: an integer vector, a string, and a
        // numeric. Each binding should get exactly one hint whose
        // label mentions the inferred mode.
        let src = "x <- 1:10\nname <- \"hello\"\nd <- 1.5\n";
        let hints = inlay_hints(src);
        assert_eq!(hints.len(), 3, "got {:?}", hints);

        // Every hint must be a TYPE hint with left padding (so it
        // renders as `x : <type>` rather than `x: <type>`).
        for h in &hints {
            assert_eq!(h.kind, Some(InlayHintKind::TYPE));
            assert_eq!(h.padding_left, Some(true));
            assert_eq!(h.padding_right, None);
        }

        // The first hint sits right after `x` at line 0, col 1.
        assert_eq!(hints[0].position.line, 0);
        assert_eq!(hints[0].position.character, 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert!(
                s.contains("integer"),
                "expected integer in label, got: {}",
                s
            ),
            other => panic!("expected String label, got {:?}", other),
        }

        // The second hint sits right after `name` at line 1, col 4.
        assert_eq!(hints[1].position.line, 1);
        assert_eq!(hints[1].position.character, 4);
        match &hints[1].label {
            InlayHintLabel::String(s) => assert!(
                s.contains("character"),
                "expected character in label, got: {}",
                s
            ),
            other => panic!("expected String label, got {:?}", other),
        }
    }

    #[test]
    fn inlay_hints_skip_opaque_types() {
        // A call to an unknown function resolves to `Mode::Opaque`
        // ("we don't know"), so `result` must NOT get a hint: showing
        // `: opaque<len=?>?NA?` next to every unknown binding would
        // just be visual noise. We bind a known integer alongside so
        // we can confirm the walker still runs and emits hints for
        // the non-opaque binding.
        let src = "result <- some_unknown_function()\nx <- 1L + 2L\n";
        let hints = inlay_hints(src);
        // Only `x` should produce a hint; `result` is opaque and skipped.
        // Each hint's position is right after its identifier:
        //   `result` is at col 0..6 -> hint at col 6 (line 0)
        //   `x`      is at col 0..1 -> hint at col 1 (line 1)
        let has_hint_for_result = hints
            .iter()
            .any(|h| h.position.line == 0 && h.position.character == 6);
        let has_hint_for_x = hints
            .iter()
            .any(|h| h.position.line == 1 && h.position.character == 1);
        assert!(
            !has_hint_for_result,
            "result is opaque and should NOT get a hint, got: {:?}",
            hints
        );
        assert!(
            has_hint_for_x,
            "x is integer and SHOULD get a hint, got: {:?}",
            hints
        );
    }

    #[test]
    fn inlay_hints_label_starts_with_colon_space() {
        // The hint label should look like a type annotation, so it
        // must start with `: ` to render as `x : integer<...>`.
        let src = "x <- 1L\n";
        let hints = inlay_hints(src);
        assert_eq!(hints.len(), 1);
        match &hints[0].label {
            InlayHintLabel::String(s) => {
                assert!(s.starts_with(": "), "expected ': ' prefix, got: {}", s);
                assert!(
                    s.contains("integer"),
                    "expected integer mode in label, got: {}",
                    s
                );
            }
            other => panic!("expected String label, got {:?}", other),
        }
    }

    #[test]
    fn inlay_hints_position_at_end_of_identifier() {
        // For `my_var <- 1L`, the hint must land at col 6 (the byte
        // right after the 6-character `my_var`), so the editor
        // renders `my_var : integer<...> <- 1L`.
        let src = "my_var <- 1L\n";
        let hints = inlay_hints(src);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].position.line, 0);
        assert_eq!(
            hints[0].position.character,
            "my_var".len() as u32,
            "hint should land right after the identifier"
        );
    }

    #[test]
    fn inlay_hints_for_function_definition() {
        // `add <- function(a, b) a + b` binds `add` to a function.
        // The walker should emit a hint at the end of `add` (col 3)
        // whose label identifies a function type.
        let src = "add <- function(a, b) a + b\n";
        let hints = inlay_hints(src);
        assert_eq!(hints.len(), 1, "got {:?}", hints);
        assert_eq!(hints[0].position.line, 0);
        assert_eq!(hints[0].position.character, 3);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert!(
                s.contains("function"),
                "expected function in label, got: {}",
                s
            ),
            other => panic!("expected String label, got {:?}", other),
        }
    }

    // ---- completion helpers ----

    /// Helper: parse + check a snippet and return the completion
    /// items for a given cursor position and trigger context. Mirrors
    /// what the `completion` LSP method does, minus the async state
    /// lookup.
    fn completions(
        src: &str,
        position: Position,
        context: Option<CompletionContext>,
    ) -> Vec<CompletionItem> {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let mut checker = ry_checker::Checker::new("test.R");
        let (_, scope) = checker.check_with_scope(&file);
        collect_completions(src, position, &context, &scope)
    }

    /// Build a `CompletionContext` for a given trigger character.
    /// Used by the `$`-triggered test to mimic what the editor sends
    /// right after the user types `$`.
    fn trigger_context(ch: &str) -> Option<CompletionContext> {
        Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
            trigger_character: Some(ch.to_string()),
        })
    }

    #[test]
    fn extract_last_identifier_basic() {
        // The variable name sits at the end of the input; the helper
        // must scan back to its start, stopping at the first non-ident
        // character.
        assert_eq!(extract_last_identifier("mtcars").as_deref(), Some("mtcars"));
        assert_eq!(extract_last_identifier("df$col").as_deref(), Some("col"));
        assert_eq!(
            extract_last_identifier("foo.bar_baz").as_deref(),
            Some("foo.bar_baz")
        );
        // Trailing whitespace / `$` are not stripped here; the caller
        // (`collect_completions`) handles that. So a trailing `$`
        // produces `None` because `$` is not an identifier character.
        assert_eq!(extract_last_identifier("mtcars$"), None);
        assert_eq!(extract_last_identifier(""), None);
        assert_eq!(extract_last_identifier("(1 + 2)"), None);
    }

    // ---- signature help helpers ----

    #[test]
    fn find_enclosing_call_basic_round() {
        // `round(` with the cursor right after the `(` (col 6): the
        // enclosing call is `round`, and no comma has been typed yet
        // so the active parameter is 0.
        let text = "round(\n";
        let (name, active) = find_enclosing_call(text, 0, 6).expect("should find call");
        assert_eq!(name, "round");
        assert_eq!(active, 0);
    }

    #[test]
    fn find_enclosing_call_counts_commas() {
        // `round(x, ` with the cursor at col 9 (after the comma + the
        // space): one comma has been typed, so the active parameter is
        // 1 (the second parameter, `digits`).
        let text = "round(x, \n";
        let (name, active) = find_enclosing_call(text, 0, 9).expect("should find call");
        assert_eq!(name, "round");
        assert_eq!(active, 1);
    }

    #[test]
    fn find_enclosing_call_skips_nested_calls() {
        // `outer(inner(1, 2), ` with the cursor at the trailing
        // space: the nearest enclosing call is `outer` (the inner
        // `inner(1, 2)` is closed), and only the top-level comma
        // (after the inner call) counts toward `outer`'s active
        // parameter, so it should be 1.
        let text = "outer(inner(1, 2), \n";
        let (name, active) = find_enclosing_call(text, 0, 18).expect("should find call");
        assert_eq!(name, "outer");
        assert_eq!(
            active, 1,
            "only the top-level comma should count, not the inner call's comma"
        );
    }

    #[test]
    fn find_enclosing_call_returns_none_outside_call() {
        // No `(` before the cursor: not inside a call.
        let text = "x <- 1\n";
        assert_eq!(find_enclosing_call(text, 0, 4), None);
    }

    #[test]
    fn find_enclosing_call_returns_none_for_non_ident_func() {
        // The text before the `(` is `(1 + 2) + (` which is not a
        // function call (no identifier before the `(`). The helper
        // must return `None` rather than treat the `(` as a call.
        let text = "1 + (2 * 3)\n";
        assert_eq!(find_enclosing_call(text, 0, 6), None);
    }

    #[test]
    fn get_signature_returns_known_params() {
        // `round` has the conventional `x, digits` parameters; the
        // helper must surface them in order.
        let params = get_signature("round").expect("round should have a signature");
        assert_eq!(params, vec!["x", "digits"]);

        // `mean` has three formal parameters.
        let params = get_signature("mean").expect("mean should have a signature");
        assert_eq!(params, vec!["x", "trim", "na.rm"]);

        // Variadic functions collapse to `...`.
        let params = get_signature("c").expect("c should have a signature");
        assert_eq!(params, vec!["..."]);
    }

    #[test]
    fn get_signature_returns_none_for_unknown() {
        // User-defined functions aren't in the curated table.
        assert!(get_signature("my_helper").is_none());
        assert!(get_signature("").is_none());
    }

    #[test]
    fn signature_help_label_and_active_param() {
        // End-to-end test of the signature-help logic at the helper
        // level: locate the enclosing call, look up the signature,
        // and verify the resulting label and active-parameter
        // highlight. We exercise the same helpers the LSP handler
        // uses so the test stays accurate even though the handler is
        // async and stateful.
        //
        // To avoid fragile byte-counting, we find the comma's position
        // dynamically and place the cursor right after it. `round(x, `
        // has one top-level comma => active param 1 (`digits`).
        let text = "round(x, ";
        let comma = text.find(',').expect("snippet should have a comma");
        let (name, active) = find_enclosing_call(text, 0, comma + 1).expect("should find call");
        assert_eq!(name, "round");
        assert_eq!(active, 1);

        let params = get_signature(&name).expect("round should have a signature");
        let label = format!("{}({})", name, params.join(", "));
        assert_eq!(label, "round(x, digits)");
        // The active parameter must be clamped to the parameter list
        // length: with 2 params and active=1, the highlight should
        // land on `digits`.
        let active_param = if active < params.len() {
            Some(active as u32)
        } else {
            None
        };
        assert_eq!(active_param, Some(1));
    }

    #[test]
    fn signature_help_clamps_when_past_last_param() {
        // When the user has typed more commas than there are formal
        // parameters (e.g. `round(1, 2, 3, `), the active-parameter
        // index should clamp to `None` so the editor clears the
        // highlight instead of pointing at a non-existent parameter.
        // `round` has 2 params; typing 3 commas puts the cursor on a
        // 4th parameter that doesn't exist.
        let text = "round(1, 2, 3, \n";
        // After the third comma (byte 14): active param 3.
        let (_, active) = find_enclosing_call(text, 0, 14).expect("should find call");
        assert_eq!(active, 3);
        let params = get_signature("round").expect("round should have a signature");
        let active_param = if active < params.len() {
            Some(active as u32)
        } else {
            None
        };
        assert_eq!(
            active_param, None,
            "active param should clamp to None past the last formal"
        );
    }

    #[test]
    fn common_r_completions_includes_keywords_and_functions() {
        // The curated list must surface a handful of keywords (so the
        // popup helps users start a function definition / loop) and
        // common base-R functions (so `c`, `list`, `mean` show up even
        // when the user has no bindings yet). Every entry must carry a
        // non-empty detail string and a kind.
        let items = common_r_completions();
        // Sanity: the list is non-empty but focused.
        assert!(!items.is_empty(), "curated list should not be empty");
        assert!(
            items.len() <= 50,
            "curated list should stay focused, got {} entries",
            items.len()
        );
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        // A representative keyword and a representative function.
        assert!(
            labels.contains(&"function"),
            "missing 'function': {:?}",
            labels
        );
        assert!(labels.contains(&"if"), "missing 'if': {:?}", labels);
        assert!(labels.contains(&"c"), "missing 'c': {:?}", labels);
        assert!(labels.contains(&"list"), "missing 'list': {:?}", labels);
        assert!(labels.contains(&"mean"), "missing 'mean': {:?}", labels);
        // Every entry must have a kind and a detail.
        for it in &items {
            assert!(it.kind.is_some(), "entry {:?} missing kind", it.label);
            assert!(
                it.detail.as_deref().is_some_and(|d| !d.is_empty()),
                "entry {:?} missing/empty detail",
                it.label
            );
        }
        // The 'function' entry must be classified as a KEYWORD (R
        // treats it as a keyword, not a function call), and 'c' as a
        // FUNCTION.
        let function_item = items.iter().find(|i| i.label == "function").unwrap();
        assert_eq!(function_item.kind, Some(CompletionItemKind::KEYWORD));
        let c_item = items.iter().find(|i| i.label == "c").unwrap();
        assert_eq!(c_item.kind, Some(CompletionItemKind::FUNCTION));
    }

    #[test]
    fn completions_for_scope_variables_and_keywords() {
        // Generic (non-triggered) completion must include the user's
        // in-scope bindings AND the curated keyword/function list.
        // Bindings get a VARIABLE or FUNCTION kind; the curated
        // keywords keep their KEYWORD / FUNCTION kind. Duplicate
        // labels (e.g. a user `c <- ...` vs the curated `c`) must be
        // collapsed by `dedup_by`.
        let src = "x <- 1L + 2L\nname <- \"hi\"\n";
        // Cursor on line 2, col 0 (a fresh line). No trigger.
        let pos = Position {
            line: 2,
            character: 0,
        };
        let items = completions(src, pos, None);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        // In-scope bindings.
        assert!(labels.contains(&"x"), "missing x: {:?}", labels);
        assert!(labels.contains(&"name"), "missing name: {:?}", labels);
        // Curated keywords / functions.
        assert!(labels.contains(&"if"), "missing if: {:?}", labels);
        assert!(
            labels.contains(&"function"),
            "missing function: {:?}",
            labels
        );
        assert!(labels.contains(&"mean"), "missing mean: {:?}", labels);
        // Dedup: 'c' should appear at most once even though both the
        // scope (no user `c` here) and the curated list could
        // contribute. This guards the dedup path against future
        // changes that add a 'c' to the scope.
        let c_count = labels.iter().filter(|&&l| l == "c").count();
        assert_eq!(c_count, 1, "'c' should appear exactly once: {:?}", labels);
        // 'x' must be a VARIABLE; the curated 'function' must be a
        // KEYWORD.
        let x_item = items.iter().find(|i| i.label == "x").unwrap();
        assert_eq!(x_item.kind, Some(CompletionItemKind::VARIABLE));
        let function_item = items.iter().find(|i| i.label == "function").unwrap();
        assert_eq!(function_item.kind, Some(CompletionItemKind::KEYWORD));
        // The list must be sorted alphabetically by label.
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(labels, sorted, "completions should be sorted by label");
    }

    #[test]
    fn completions_for_dollar_trigger_returns_columns() {
        // When `$` is the trigger, the popup must show ONLY the
        // column names of the variable before the `$`. We use a
        // `list(a = <int>, b = <chr>)` literal so the checker infers
        // a `ColumnSchema` with columns `a` and `b`. Each column item
        // must be a FIELD whose detail surfaces its inferred type.
        let src = "df <- list(a = 1L, b = \"x\")\ndf$\n";
        // Cursor right after the `$` on line 1 (col 3: 'd','f','$').
        let pos = Position {
            line: 1,
            character: 3,
        };
        let items = completions(src, pos, trigger_context("$"));
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"a"), "missing column a: {:?}", labels);
        assert!(labels.contains(&"b"), "missing column b: {:?}", labels);
        // No scope variables / keywords should leak into the column
        // popup.
        assert!(
            !labels.contains(&"df"),
            "df should not appear in column completions: {:?}",
            labels
        );
        assert!(
            !labels.contains(&"if"),
            "keywords should not appear in column completions: {:?}",
            labels
        );
        // Every item must be a FIELD (column) with a non-empty detail.
        for it in &items {
            assert_eq!(
                it.kind,
                Some(CompletionItemKind::FIELD),
                "column {:?} should be FIELD",
                it.label
            );
            assert!(
                it.detail.as_deref().is_some_and(|d| !d.is_empty()),
                "column {:?} missing detail",
                it.label
            );
        }
        // Column 'a' is integer and 'b' is character; the detail
        // strings should reflect that so the popup shows the type
        // next to the name.
        let a_item = items.iter().find(|i| i.label == "a").unwrap();
        let a_detail = a_item.detail.as_deref().unwrap();
        assert!(
            a_detail.contains("integer"),
            "column a should be integer, got: {}",
            a_detail
        );
        let b_item = items.iter().find(|i| i.label == "b").unwrap();
        let b_detail = b_item.detail.as_deref().unwrap();
        assert!(
            b_detail.contains("character"),
            "column b should be character, got: {}",
            b_detail
        );
    }

    #[test]
    fn completions_for_dollar_trigger_without_schema_returns_empty() {
        // If the variable before the `$` has no `ColumnSchema` (e.g.
        // a plain integer vector), the `$`-triggered popup must
        // return an empty list rather than fall through to the
        // generic in-scope list. Falling through would dump every
        // binding where the user expects column names.
        let src = "x <- 1L + 2L\nx$\n";
        let pos = Position {
            line: 1,
            character: 2,
        };
        let items = completions(src, pos, trigger_context("$"));
        assert!(
            items.is_empty(),
            "expected no completions for non-data-frame $, got: {:?}",
            items
        );
    }

    // ---- references (find all references) helpers ----

    /// Helper: parse a snippet and return the references to `name`
    /// within it. Mirrors what the `references` LSP method does for a
    /// single document, minus the async state lookup. Uses
    /// `include_declaration` to control whether definition sites are
    /// included.
    fn references_in(src: &str, name: &str, include_declaration: bool) -> Vec<Location> {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        find_references_in_file(&file, name, &uri, src, include_declaration)
    }

    #[test]
    fn references_finds_variable_usages_in_same_file() {
        // `x` is defined once and read twice (in the RHS of `y` and in
        // `z`). With include_declaration = false, only the two reads
        // should be returned.
        let src = "x <- 1L\ny <- x + 1\nz <- x * 2\n";
        let locs = references_in(src, "x", false);
        assert_eq!(locs.len(), 2, "expected 2 references to x, got {:?}", locs);
        // The two references live on lines 1 and 2 (0-indexed).
        let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
        assert!(
            lines.contains(&1),
            "expected a reference on line 1: {:?}",
            lines
        );
        assert!(
            lines.contains(&2),
            "expected a reference on line 2: {:?}",
            lines
        );
        // Each reference must cover exactly the identifier "x" (1 char
        // wide), not a zero-width or multi-char range.
        for loc in &locs {
            assert_eq!(
                loc.range.end.character - loc.range.start.character,
                1,
                "expected 1-char wide range for 'x'"
            );
        }
    }

    #[test]
    fn references_finds_function_call_sites() {
        // `add` is defined as a function and called twice. With
        // include_declaration = false, only the two call sites on
        // lines 1 and 2 should be returned.
        let src = "add <- function(a, b) a + b\nadd(1, 2)\nadd(3, 4)\n";
        let locs = references_in(src, "add", false);
        assert_eq!(locs.len(), 2, "expected 2 call sites, got {:?}", locs);
        let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
        assert!(lines.contains(&1), "expected a call on line 1: {:?}", lines);
        assert!(lines.contains(&2), "expected a call on line 2: {:?}", lines);
        // Each call-site range covers exactly the 3-char name "add".
        for loc in &locs {
            assert_eq!(
                loc.range.end.character - loc.range.start.character,
                3,
                "expected 3-char wide range for 'add'"
            );
            assert_eq!(loc.range.start.character, 0, "calls start at col 0");
        }
    }

    #[test]
    fn references_include_declaration_flag() {
        // `x` is defined once (line 0) and read once (line 1).
        let src = "x <- 1L\nx + 1\n";
        // With include_declaration = true: the definition (line 0) AND
        // the read (line 1) => 2 locations.
        let locs_with = references_in(src, "x", true);
        assert_eq!(
            locs_with.len(),
            2,
            "expected 2 locations with declaration, got {:?}",
            locs_with
        );
        // With include_declaration = false: only the read (line 1) =>
        // 1 location, and it must NOT be the definition on line 0.
        let locs_without = references_in(src, "x", false);
        assert_eq!(
            locs_without.len(),
            1,
            "expected 1 location without declaration, got {:?}",
            locs_without
        );
        assert_eq!(
            locs_without[0].range.start.line, 1,
            "the lone reference must be the read on line 1"
        );
    }

    #[test]
    fn references_across_multiple_files() {
        // Simulate two open documents: a.R defines `helper`, b.R calls
        // it. This mirrors how `references` walks `self.state.docs`
        // across all open documents (we drive `find_references_in_file`
        // directly for each parsed file since the async state is not
        // reachable from a unit test).
        let src_a = "helper <- function() 1L\n";
        let src_b = "helper()\n";
        let mut parser = RParser::new().unwrap();
        let file_a = parser.parse("a.R", src_a).unwrap();
        let file_b = parser.parse("b.R", src_b).unwrap();
        let uri_a = Url::parse("file:///tmp/a.R").unwrap();
        let uri_b = Url::parse("file:///tmp/b.R").unwrap();

        // include_declaration = true so the definition in a.R counts.
        let mut all = Vec::new();
        all.extend(find_references_in_file(
            &file_a, "helper", &uri_a, src_a, true,
        ));
        all.extend(find_references_in_file(
            &file_b, "helper", &uri_b, src_b, true,
        ));

        // One definition in a.R + one call in b.R => 2 locations.
        assert_eq!(
            all.len(),
            2,
            "expected 2 locations across files, got {:?}",
            all
        );
        // The locations must come from different URIs (one per file).
        let uris: Vec<&Url> = all.iter().map(|l| &l.uri).collect();
        assert!(
            uris.contains(&&uri_a),
            "missing location in a.R: {:?}",
            uris
        );
        assert!(
            uris.contains(&&uri_b),
            "missing location in b.R: {:?}",
            uris
        );
    }

    #[test]
    fn references_finds_usages_inside_nested_scopes() {
        // `data` is read inside an anonymous function body (via index
        // `data[1]`) and inside a for-loop body (via `print(data)`).
        // The walker must recurse into both nested scopes.
        let src = "data <- c(1, 2, 3)\nf <- function() {\n  data[1]\n}\nfor (i in 1:3) {\n  print(data)\n}\n";
        let locs = references_in(src, "data", false);
        // Two reads: inside the function body (line 2) and inside the
        // for-loop body (line 5). The definition on line 0 is excluded
        // because include_declaration is false.
        assert_eq!(
            locs.len(),
            2,
            "expected 2 nested references, got {:?}",
            locs
        );
        let lines: Vec<u32> = locs.iter().map(|l| l.range.start.line).collect();
        assert!(
            lines.contains(&2),
            "expected a reference on line 2: {:?}",
            lines
        );
        assert!(
            lines.contains(&5),
            "expected a reference on line 5: {:?}",
            lines
        );
    }

    #[test]
    fn references_returns_empty_for_undefined_name() {
        // No occurrences of `does_not_exist` anywhere.
        let src = "x <- 1L\ny <- x + 1\n";
        let locs = references_in(src, "does_not_exist", true);
        assert!(locs.is_empty(), "expected no references, got {:?}", locs);
    }

    #[test]
    fn references_self_referencing_assignment() {
        // `x <- x + 1` references `x` on the RHS even though the LHS is
        // a definition. With include_declaration = true the LHS counts
        // too, giving 2 locations; with false only the RHS read counts.
        let src = "x <- x + 1\n";
        let locs_with = references_in(src, "x", true);
        assert_eq!(locs_with.len(), 2, "got {:?}", locs_with);
        let locs_without = references_in(src, "x", false);
        assert_eq!(locs_without.len(), 1, "got {:?}", locs_without);
        // The lone reference (RHS) is at col 5 ("x <- x...").
        assert_eq!(locs_without[0].range.start.character, 5);
    }

    // ---- workspace symbols helpers ----

    /// Helper: parse + check a snippet and return its top-level
    /// `DocumentSymbol`s, then flatten them into `SymbolInformation`s
    /// attached to the given URI. Mirrors what the `symbol` LSP method
    /// does for a single document, minus the async state lookup and
    /// the cross-document iteration / query filter.
    fn workspace_symbols(src: &str, uri: &Url) -> Vec<SymbolInformation> {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let mut checker = ry_checker::Checker::new("test.R");
        let (_, scope) = checker.check_with_scope(&file);
        let doc_symbols = collect_symbols(&file.stmts, src, Some(&scope));
        flatten_symbols_to_symbol_info(doc_symbols, uri)
    }

    #[test]
    fn workspace_symbols_flatten_tree_with_container_names() {
        // The canonical example: a function `add` (with parameters
        // `a` and `b` that become nested children), a variable
        // `result`, and a variable `name`. Flattening must produce
        // one `SymbolInformation` per node (function + each param +
        // each top-level variable) and propagate the parent's name
        // into each child's `container_name`.
        let src = "add <- function(a, b) a + b\nresult <- add(1, 2)\nname <- \"hello\"\n";
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let symbols = workspace_symbols(src, &uri);

        // 1 function (add) + 2 params (a, b) + 2 variables (result,
        // name) => 5 flattened symbols.
        assert_eq!(symbols.len(), 5, "got {:?}", symbols);

        // Every symbol must point at the file we passed in.
        for s in &symbols {
            assert_eq!(s.location.uri, uri, "wrong uri for {:?}", s.name);
        }

        // Top-level symbols have `container_name = None`; the
        // function's parameters inherit `container_name = "add"`.
        // Build a name -> container_name lookup to assert each.
        let container_of = |name: &str| -> Option<String> {
            symbols
                .iter()
                .find(|s| s.name == name)
                .and_then(|s| s.container_name.clone())
        };
        assert_eq!(container_of("add"), None, "add is top-level");
        assert_eq!(container_of("result"), None, "result is top-level");
        assert_eq!(container_of("name"), None, "name is top-level");
        assert_eq!(
            container_of("a"),
            Some("add".to_string()),
            "a is a parameter of add"
        );
        assert_eq!(
            container_of("b"),
            Some("add".to_string()),
            "b is a parameter of add"
        );

        // The function symbol must be classified as FUNCTION, the
        // parameters and variables as VARIABLE.
        let kind_of =
            |name: &str| -> SymbolKind { symbols.iter().find(|s| s.name == name).unwrap().kind };
        assert_eq!(kind_of("add"), SymbolKind::FUNCTION);
        assert_eq!(kind_of("a"), SymbolKind::VARIABLE);
        assert_eq!(kind_of("b"), SymbolKind::VARIABLE);
        assert_eq!(kind_of("result"), SymbolKind::VARIABLE);
        assert_eq!(kind_of("name"), SymbolKind::VARIABLE);

        // The function symbol's location range must cover exactly the
        // 3-character identifier `add` at line 0, col 0 (this is the
        // `selection_range` propagated from the `DocumentSymbol`).
        let add = symbols.iter().find(|s| s.name == "add").unwrap();
        assert_eq!(add.location.range.start.line, 0);
        assert_eq!(add.location.range.start.character, 0);
        assert_eq!(add.location.range.end.line, 0);
        assert_eq!(add.location.range.end.character, 3);
    }

    #[test]
    fn workspace_symbols_filter_case_insensitive_substring() {
        // The `symbol` handler retains a symbol when its name contains
        // the query as a case-insensitive substring. We exercise the
        // filter inline (the handler does `name.to_lowercase().contains
        // (&query.to_lowercase())`) so the test pins the exact
        // matching rule: 'RES' must match 'result' but not 'add'.
        let src = "add <- function(a, b) a + b\nresult <- add(1, 2)\n";
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let mut symbols = workspace_symbols(src, &uri);

        // Sanity: without filtering we get add, a, b, result.
        assert_eq!(symbols.len(), 4, "got {:?}", symbols);

        // Apply the same filter the handler uses.
        let query = "RES".to_string();
        let query_lower = query.to_lowercase();
        symbols.retain(|s| s.name.to_lowercase().contains(&query_lower));

        // Only `result` contains "res" case-insensitively.
        assert_eq!(symbols.len(), 1, "got {:?}", symbols);
        assert_eq!(symbols[0].name, "result");

        // An empty query must NOT filter anything (the handler
        // special-cases this), so re-fetch and check.
        let symbols_all = workspace_symbols(src, &uri);
        assert_eq!(
            symbols_all.len(),
            4,
            "empty query should return all symbols"
        );
    }

    #[test]
    fn workspace_symbols_empty_when_no_bindings() {
        // A bare expression with no assignments produces no
        // `DocumentSymbol`s and therefore no `SymbolInformation`s.
        let src = "1L + 2L\n";
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let symbols = workspace_symbols(src, &uri);
        assert!(symbols.is_empty(), "expected no symbols, got {:?}", symbols);
    }

    // ---- rename helpers ----

    #[test]
    fn rename_edits_single_file_includes_declaration_and_usages() {
        // `x` is defined once (line 0) and read once (line 1, col 4
        // in `y <- x + 1`). Renaming `x` to `new_x` must produce 2
        // edits in the same file: one for the declaration and one
        // for the usage. Each edit must replace the 1-character
        // identifier span with the new name.
        let src = "x <- 1L\ny <- x + 1\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let edit = build_rename_edits(&[("test.R", &file, src)], "x", "new_x");

        let changes = edit.changes.expect("should have changes");
        // All edits land in the same file (one entry in the map).
        assert_eq!(changes.len(), 1, "got {:?}", changes);
        let uri = path_to_uri("test.R");
        let edits = changes.get(&uri).expect("should have edits for test.R");
        assert_eq!(edits.len(), 2, "got {:?}", edits);
        // Every edit must carry the new name and target a 1-char range.
        for e in edits {
            assert_eq!(e.new_text, "new_x");
            assert_eq!(
                e.range.end.character - e.range.start.character,
                1,
                "expected 1-char wide range for 'x'"
            );
        }
        // The two edits must cover the declaration on line 0 and the
        // usage on line 1 (the only `x` in the source).
        let lines: Vec<u32> = edits.iter().map(|e| e.range.start.line).collect();
        assert!(
            lines.contains(&0),
            "expected an edit on line 0: {:?}",
            lines
        );
        assert!(
            lines.contains(&1),
            "expected an edit on line 1: {:?}",
            lines
        );
    }

    #[test]
    fn rename_edits_across_files_group_by_uri() {
        // Simulate two open documents: a.R defines `helper`, b.R calls
        // it. Renaming `helper` to `h` must produce one edit in each
        // file, grouped under separate URIs in the `changes` map.
        // We use absolute paths so `path_to_uri` produces distinct
        // `file://` URIs (relative paths would all collapse to the
        // `file:///unknown` fallback).
        let src_a = "helper <- function() 1L\n";
        let src_b = "helper()\n";
        let mut parser = RParser::new().unwrap();
        let file_a = parser.parse("/tmp/a.R", src_a).unwrap();
        let file_b = parser.parse("/tmp/b.R", src_b).unwrap();
        let edit = build_rename_edits(
            &[("/tmp/a.R", &file_a, src_a), ("/tmp/b.R", &file_b, src_b)],
            "helper",
            "h",
        );

        let changes = edit.changes.expect("should have changes");
        // One entry per file.
        assert_eq!(changes.len(), 2, "got {:?}", changes);
        let uri_a = path_to_uri("/tmp/a.R");
        let uri_b = path_to_uri("/tmp/b.R");
        let edits_a = changes.get(&uri_a).expect("a.R should have edits");
        let edits_b = changes.get(&uri_b).expect("b.R should have edits");
        // One edit per file (the definition in a.R, the call in b.R).
        assert_eq!(edits_a.len(), 1, "got {:?}", edits_a);
        assert_eq!(edits_b.len(), 1, "got {:?}", edits_b);
        // Every edit must replace with the new name and target a
        // 7-character span (the length of `helper`).
        for e in edits_a.iter().chain(edits_b.iter()) {
            assert_eq!(e.new_text, "h");
            assert_eq!(
                e.range.end.character - e.range.start.character,
                "helper".len() as u32
            );
        }
    }

    #[test]
    fn rename_edits_unknown_name_yields_empty_changes() {
        // Renaming a name that doesn't exist anywhere must produce an
        // empty `changes` map (still `Some`, just with no entries).
        let src = "x <- 1L\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let edit = build_rename_edits(&[("test.R", &file, src)], "does_not_exist", "y");
        let changes = edit.changes.expect("should still be Some(empty)");
        assert!(changes.is_empty(), "got {:?}", changes);
    }

    // ---- prepareRename helpers ----

    #[test]
    fn prepare_rename_returns_identifier_range() {
        // Cursor on 'v' inside `my_var` (line 0, col 2): the helper
        // must return the full identifier name AND a range covering
        // exactly `my_var` (cols 0..6 on line 0).
        let text = "my_var <- 1L\n";
        let (name, range) =
            find_identifier_range_at_position(text, 0, 2).expect("should find identifier");
        assert_eq!(name, "my_var");
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 0);
        assert_eq!(range.end.character, "my_var".len() as u32);
    }

    #[test]
    fn prepare_rename_returns_none_for_keywords_and_operators() {
        // R keywords are not renameable bindings: `if` must yield
        // `None` so the editor does not offer a rename UI on it.
        let text = "if (TRUE) { x <- 1 }\n";
        assert_eq!(
            find_identifier_range_at_position(text, 0, 1),
            None,
            "keyword 'if' must not be renameable"
        );
        // Operators / whitespace must also yield `None`.
        let text = "x <- 1L\n";
        assert_eq!(
            find_identifier_range_at_position(text, 0, 2),
            None,
            "operator '<-' must not be renameable"
        );
        // Pure numbers must yield `None`.
        let text = "x <- 123\n";
        assert_eq!(
            find_identifier_range_at_position(text, 0, 5),
            None,
            "pure numbers must not be renameable"
        );
    }

    #[test]
    fn prepare_rename_at_end_of_word_still_resolves() {
        // Cursor right after the last identifier character (a common
        // transient state when the user just clicked at the end of
        // a word): the helper must still resolve the identifier. We
        // place the cursor on the space after `my_var` (col 6).
        let text = "my_var <- 1L\n";
        let (name, range) = find_identifier_range_at_position(text, 0, 6)
            .expect("should find identifier at end of word");
        assert_eq!(name, "my_var");
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.character, "my_var".len() as u32);
    }

    // ---- document highlight helpers ----

    /// Helper: parse a snippet and return the `DocumentHighlight`s for
    /// `name`. Mirrors what the `document_highlight` LSP method does,
    /// minus the async state lookup. Order of the returned highlights
    /// follows source order (top-to-bottom).
    fn doc_highlights(src: &str, name: &str) -> Vec<DocumentHighlight> {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        collect_document_highlights(&file, name, src)
    }

    #[test]
    fn document_highlight_classifies_write_and_read() {
        // `x` is written at line 0 (assignment target) and read on
        // lines 1 and 2 (RHS of `y` and `z`). The WRITE must land on
        // line 0 and the two READs on lines 1 and 2.
        let src = "x <- 1L\ny <- x + 1\nz <- x * 2\n";
        let hl = doc_highlights(src, "x");
        assert_eq!(hl.len(), 3, "got {:?}", hl);

        // Exactly one WRITE at line 0, covering exactly the 1-char
        // identifier `x` at col 0.
        let writes: Vec<&DocumentHighlight> = hl
            .iter()
            .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
            .collect();
        assert_eq!(writes.len(), 1, "expected one WRITE: {:?}", hl);
        assert_eq!(writes[0].range.start.line, 0);
        assert_eq!(writes[0].range.start.character, 0);
        assert_eq!(writes[0].range.end.character, 1);

        // Two READs on lines 1 and 2.
        let reads: Vec<&DocumentHighlight> = hl
            .iter()
            .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
            .collect();
        assert_eq!(reads.len(), 2, "expected two READs: {:?}", hl);
        let read_lines: Vec<u32> = reads.iter().map(|h| h.range.start.line).collect();
        assert!(
            read_lines.contains(&1),
            "expected READ on line 1: {:?}",
            read_lines
        );
        assert!(
            read_lines.contains(&2),
            "expected READ on line 2: {:?}",
            read_lines
        );
    }

    #[test]
    fn document_highlight_self_referencing_assignment_has_write_and_read() {
        // `x <- x + 1` writes `x` on the LHS (col 0) and reads `x` on
        // the RHS (col 5). Both must be highlighted with the right
        // kinds on the same line.
        let src = "x <- x + 1\n";
        let hl = doc_highlights(src, "x");
        assert_eq!(hl.len(), 2, "got {:?}", hl);
        // Find the WRITE (LHS at col 0) and the READ (RHS at col 5).
        let write = hl
            .iter()
            .find(|h| h.kind == Some(DocumentHighlightKind::WRITE))
            .expect("expected a WRITE");
        assert_eq!(write.range.start.line, 0);
        assert_eq!(write.range.start.character, 0);
        let read = hl
            .iter()
            .find(|h| h.kind == Some(DocumentHighlightKind::READ))
            .expect("expected a READ");
        assert_eq!(read.range.start.line, 0);
        assert_eq!(read.range.start.character, 5);
    }

    #[test]
    fn document_highlight_finds_occurrences_inside_nested_scopes() {
        // `data` is written at line 0 and read inside a function body
        // (line 2) and inside a for-loop body (line 5). The walker
        // must recurse into both nested scopes.
        let src = "data <- c(1, 2, 3)\nf <- function() {\n  data[1]\n}\nfor (i in 1:3) {\n  print(data)\n}\n";
        let hl = doc_highlights(src, "data");
        // 1 WRITE (line 0) + 2 READs (lines 2 and 5) = 3 highlights.
        assert_eq!(hl.len(), 3, "got {:?}", hl);
        let read_lines: Vec<u32> = hl
            .iter()
            .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
            .map(|h| h.range.start.line)
            .collect();
        assert!(
            read_lines.contains(&2),
            "expected READ on line 2: {:?}",
            read_lines
        );
        assert!(
            read_lines.contains(&5),
            "expected READ on line 5: {:?}",
            read_lines
        );
    }

    #[test]
    fn document_highlight_returns_empty_for_unknown_name() {
        let src = "x <- 1L\ny <- x + 1\n";
        let hl = doc_highlights(src, "does_not_exist");
        assert!(hl.is_empty(), "expected no highlights, got {:?}", hl);
    }

    #[test]
    fn document_highlight_classifies_loop_variable_as_write() {
        // The loop variable `i` is re-bound each iteration, so it
        // should be classified as a WRITE. The single READ lives in
        // the loop body on line 1.
        let src = "for (i in 1:3) {\n  print(i)\n}\n";
        let hl = doc_highlights(src, "i");
        assert_eq!(hl.len(), 2, "got {:?}", hl);
        let writes: Vec<&DocumentHighlight> = hl
            .iter()
            .filter(|h| h.kind == Some(DocumentHighlightKind::WRITE))
            .collect();
        assert_eq!(
            writes.len(),
            1,
            "expected one WRITE for the loop var: {:?}",
            hl
        );
        let reads: Vec<&DocumentHighlight> = hl
            .iter()
            .filter(|h| h.kind == Some(DocumentHighlightKind::READ))
            .collect();
        assert_eq!(reads.len(), 1, "expected one READ in the body: {:?}", hl);
        assert_eq!(reads[0].range.start.line, 1);
    }

    // ---- folding range helpers ----

    /// Helper: parse a snippet and return its folding ranges. Mirrors
    /// what the `folding_range` LSP method does, minus the async state
    /// lookup. Ranges are returned in source order.
    fn folding_ranges(src: &str) -> Vec<FoldingRange> {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        collect_folding_ranges(&file, src)
    }

    #[test]
    fn folding_range_for_multiline_function_body() {
        // A function whose body spans multiple lines must produce a
        // folding range covering the body. The named function pattern
        // `f <- function() { ... }` is modeled by the parser as an
        // `Assign` with an `Expr::Function` value, so the recursive
        // `collect_folding_from_expr` must find the function-literal
        // span. The body starts on line 0 and ends on line 2.
        let src = "f <- function() {\n  x <- 1L\n  x\n}\n";
        let ranges = folding_ranges(src);
        assert!(
            !ranges.is_empty(),
            "expected at least one range, got {:?}",
            ranges
        );
        // At least one range must start at line 0 and end at line 3.
        let covers_func = ranges.iter().any(|r| {
            r.start_line == 0 && r.end_line == 3 && r.kind == Some(FoldingRangeKind::Region)
        });
        assert!(
            covers_func,
            "expected a range covering the function body (0..3), got {:?}",
            ranges
        );
        // Every range must be `Region`-kinded and span at least 2 lines.
        for r in &ranges {
            assert_eq!(r.kind, Some(FoldingRangeKind::Region));
            assert!(
                r.end_line > r.start_line,
                "expected multi-line range: {:?}",
                r
            );
        }
    }

    #[test]
    fn folding_range_for_if_else_block() {
        // An `if`/`else` block whose body spans multiple lines must
        // produce a folding range. The `if` statement spans lines 0..2.
        let src = "if (x > 0) {\n  print(\"pos\")\n} else {\n  print(\"nonpos\")\n}\n";
        let ranges = folding_ranges(src);
        assert!(
            !ranges.is_empty(),
            "expected at least one range, got {:?}",
            ranges
        );
        // The outer `if` must cover lines 0..4 (it ends on the final
        // `}` of the `else` block).
        let covers_if = ranges.iter().any(|r| r.start_line == 0 && r.end_line == 4);
        assert!(
            covers_if,
            "expected a range covering the whole if/else (0..4), got {:?}",
            ranges
        );
    }

    #[test]
    fn folding_range_for_for_loop() {
        // A `for` loop with a multi-line body must fold from the loop
        // header line down to the closing brace.
        let src = "for (i in 1:3) {\n  print(i)\n  print(i * 2)\n}\n";
        let ranges = folding_ranges(src);
        assert!(
            !ranges.is_empty(),
            "expected at least one range, got {:?}",
            ranges
        );
        let covers_for = ranges.iter().any(|r| r.start_line == 0 && r.end_line == 3);
        assert!(
            covers_for,
            "expected a range covering the for loop (0..3), got {:?}",
            ranges
        );
    }

    #[test]
    fn folding_range_empty_for_single_line_statement() {
        // A single-line statement has no foldable region; the helper
        // must return an empty list.
        let src = "x <- 1L + 2L\ny <- x * 3L\n";
        let ranges = folding_ranges(src);
        assert!(
            ranges.is_empty(),
            "expected no folding ranges for single-line code, got {:?}",
            ranges
        );
    }

    #[test]
    fn folding_range_nested_blocks_each_get_their_own_range() {
        // A function body containing a nested multi-line `if` must
        // yield (at least) two ranges: one for the outer function and
        // one for the inner `if`. This guards the recursive walk.
        let src = "f <- function(x) {\n  if (x > 0) {\n    print(x)\n  }\n}\n";
        let ranges = folding_ranges(src);
        // We expect at least 2 ranges: the outer function body and
        // the inner if body.
        assert!(
            ranges.len() >= 2,
            "expected at least 2 ranges (function + nested if), got {:?}",
            ranges
        );
        // One range must cover the whole function (lines 0..4), and
        // another must cover the inner if (lines 1..3).
        let has_outer = ranges.iter().any(|r| r.start_line == 0 && r.end_line == 4);
        let has_inner_if = ranges.iter().any(|r| r.start_line == 1 && r.end_line == 3);
        assert!(has_outer, "missing outer function range: {:?}", ranges);
        assert!(has_inner_if, "missing inner if range: {:?}", ranges);
    }

    // ---- code action helpers ----

    /// Helper: build an LSP `Diagnostic` covering a given line range
    /// with a string code, mirroring what `diagnostic_to_lsp` produces.
    /// Used by the code-action tests so we do not have to run the full
    /// checker pipeline just to exercise the quick-fix builders.
    fn lsp_diag(line: u32, start_char: u32, end_char: u32, code: &str) -> LspDiagnostic {
        LspDiagnostic {
            range: Range {
                start: Position {
                    line,
                    character: start_char,
                },
                end: Position {
                    line,
                    character: end_char,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(code.to_string())),
            source: Some("ry".to_string()),
            message: "test diagnostic".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn code_action_ignore_line_appends_suppression_comment() {
        // The canonical case: a diagnostic on `x <- 1L + "s"` should
        // produce a quick-fix that appends
        // `  # ry: ignore[RY040]` to the end of line 0. The edit's
        // range covers the whole line (col 0 to line length) and the
        // new text is the original line plus the comment.
        let text = "x <- 1L + \"s\"\n";
        let diag = lsp_diag(0, 0, 1, "RY040");
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let action = make_ignore_action(&uri, &diag, text).expect("should produce an action");

        assert_eq!(action.title, "Ignore RY040 on this line");
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        // The action must link back to the diagnostic it fixes so the
        // editor can show the lightbulb on the right squiggle.
        assert_eq!(
            action.diagnostics.as_deref(),
            Some(std::slice::from_ref(&diag))
        );

        let edit = action.edit.expect("should have an edit");
        let changes = edit.changes.expect("should have changes");
        let edits = changes.get(&uri).expect("should have edits for the uri");
        assert_eq!(edits.len(), 1, "expected exactly one text edit");
        let te = &edits[0];
        // The range covers the whole line (col 0 to len).
        assert_eq!(te.range.start.line, 0);
        assert_eq!(te.range.start.character, 0);
        assert_eq!(te.range.end.line, 0);
        assert_eq!(
            te.range.end.character,
            "x <- 1L + \"s\"".len() as u32,
            "range should span the whole line"
        );
        // The new text is the original line plus the suppression
        // comment.
        assert_eq!(
            te.new_text, "x <- 1L + \"s\"  # ry: ignore[RY040]",
            "new text should append the ignore comment"
        );
    }

    #[test]
    fn code_action_ignore_line_skips_already_suppressed() {
        // A line that already carries an `ry: ignore` comment is fully
        // suppressed; the action must return `None` so the lightbulb
        // does not offer a redundant no-op.
        let text = "x <- 1L + \"s\"  # ry: ignore[RY040]\n";
        let diag = lsp_diag(0, 0, 1, "RY040");
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        assert!(
            make_ignore_action(&uri, &diag, text).is_none(),
            "should not offer an action for an already-suppressed line"
        );
    }

    #[test]
    fn code_action_ignore_line_handles_missing_code() {
        // A diagnostic without a code (defensive) must still produce an
        // action, with the comment omitting the `[CODE]` suffix.
        let text = "x <- bad_thing()\n";
        let mut diag = lsp_diag(0, 0, 1, "RY099");
        diag.code = None;
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let action = make_ignore_action(&uri, &diag, text).expect("should produce an action");
        let edit = action.edit.expect("should have an edit");
        let changes = edit.changes.unwrap();
        let te = &changes.get(&uri).unwrap()[0];
        assert_eq!(
            te.new_text, "x <- bad_thing()  # ry: ignore",
            "missing code should omit the [CODE] suffix"
        );
        assert_eq!(
            action.title, "Ignore this diagnostic on its line",
            "missing code should use a generic title"
        );
    }

    #[test]
    fn code_action_ignore_file_inserts_at_line_zero() {
        // The file-level action inserts `# ry: ignore-file\n` at the
        // very top of the document (a zero-width insert at (0, 0)).
        let text = "x <- 1L\ny <- 2L\n";
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        let action =
            make_ignore_file_action(&uri, text).expect("should produce a file-level action");

        assert_eq!(action.title, "Ignore all diagnostics in this file");
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        let edit = action.edit.expect("should have an edit");
        let changes = edit.changes.unwrap();
        let te = &changes.get(&uri).unwrap()[0];
        // The insert is at the very start of the file.
        assert_eq!(te.range.start.line, 0);
        assert_eq!(te.range.start.character, 0);
        assert_eq!(te.range.end.line, 0);
        assert_eq!(te.range.end.character, 0);
        assert_eq!(te.new_text, "# ry: ignore-file\n");
    }

    #[test]
    fn code_action_ignore_file_skips_already_suppressed() {
        // A file that already has `# ry: ignore-file` must not get a
        // second file-level action.
        let text = "# ry: ignore-file\nx <- 1L\n";
        let uri = Url::parse("file:///tmp/test.R").unwrap();
        assert!(
            make_ignore_file_action(&uri, text).is_none(),
            "should not offer a file-level action when one already exists"
        );
    }

    #[test]
    fn diag_code_from_lsp_extracts_string_code() {
        // ry always emits string codes; the helper must surface them.
        let diag = lsp_diag(0, 0, 1, "RY040");
        assert_eq!(diag_code_from_lsp(&diag), "RY040");
    }

    #[test]
    fn diag_code_from_lsp_handles_missing_code() {
        // A diagnostic with no code yields an empty string (not a
        // panic), so the ignore-comment builder can fall back to the
        // code-less format.
        let mut diag = lsp_diag(0, 0, 1, "RY099");
        diag.code = None;
        assert_eq!(diag_code_from_lsp(&diag), "");
    }

    // ---- selection range helpers ----

    /// Helper: parse a snippet and return the `SelectionRange` chain
    /// for a single cursor position. Mirrors what the
    /// `selection_range` LSP method does, minus the async state
    /// lookup.
    fn selection_range_at(src: &str, position: Position) -> SelectionRange {
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        build_selection_range(position, &file, src)
    }

    /// Walk a `SelectionRange` chain from narrowest to widest,
    /// returning the list of `Range`s in order. Used by the tests to
    /// assert the chain widens monotonically.
    fn chain_ranges(sel: &SelectionRange) -> Vec<Range> {
        let mut out = vec![sel.range];
        let mut cur = &sel.parent;
        while let Some(p) = cur {
            out.push(p.range);
            cur = &p.parent;
        }
        out
    }

    #[test]
    fn selection_range_chain_widens_from_identifier_to_file() {
        // For `result <- x + 1` with the cursor on `result`, the chain
        // must widen: identifier (`result`) -> enclosing statement ->
        // whole file. Each level must strictly contain the previous.
        let src = "result <- x + 1\n";
        // Cursor on 's' in 'result' (line 0, col 2).
        let pos = Position {
            line: 0,
            character: 2,
        };
        let sel = selection_range_at(src, pos);
        let ranges = chain_ranges(&sel);

        // The narrowest range must cover the identifier `result`
        // (cols 0..6 on line 0).
        assert_eq!(ranges[0].start.line, 0);
        assert_eq!(ranges[0].start.character, 0);
        assert_eq!(ranges[0].end.character, "result".len() as u32);

        // The chain must have at least 2 levels (identifier + file).
        assert!(
            ranges.len() >= 2,
            "expected at least 2 levels, got {:?}",
            ranges
        );

        // Every level must contain the cursor position.
        for r in &ranges {
            let contains = (r.start.line < pos.line
                || (r.start.line == pos.line && r.start.character <= pos.character))
                && (r.end.line > pos.line
                    || (r.end.line == pos.line && r.end.character >= pos.character));
            assert!(contains, "range {:?} does not contain cursor {:?}", r, pos);
        }

        // Each level must contain or equal the previous (monotonic
        // widening), with no two consecutive identical ranges.
        for w in ranges.windows(2) {
            assert!(
                w[0] != w[1],
                "consecutive duplicate ranges in chain: {:?}",
                w
            );
        }

        // The widest level (last) must start at (0, 0).
        let widest = ranges.last().unwrap();
        assert_eq!(widest.start.line, 0);
        assert_eq!(widest.start.character, 0);
    }

    #[test]
    fn selection_range_identifier_on_rhs() {
        // Cursor on `x` in `result <- x + 1` (the RHS read). The
        // narrowest range must be the identifier `x` (1 char), and
        // the chain must widen to the enclosing statement.
        let src = "result <- x + 1\n";
        // `x` is at line 0, col 10 (after "result <- ").
        let pos = Position {
            line: 0,
            character: 10,
        };
        let sel = selection_range_at(src, pos);
        let ranges = chain_ranges(&sel);

        // The narrowest range is the single-character `x`.
        assert_eq!(ranges[0].start.line, 0);
        assert_eq!(ranges[0].start.character, 10);
        assert_eq!(ranges[0].end.character, 11);

        // The chain widens beyond the identifier.
        assert!(
            ranges.len() >= 2,
            "expected at least 2 levels, got {:?}",
            ranges
        );
    }

    #[test]
    fn selection_range_picks_correct_statement_in_multi_line_file() {
        // In a two-statement file, the enclosing statement for a
        // cursor on line 1 must be the second statement, not the
        // first.
        let src = "x <- 1L\ny <- x + 1\n";
        // Cursor on `y` (line 1, col 0).
        let pos = Position {
            line: 1,
            character: 0,
        };
        let sel = selection_range_at(src, pos);
        let ranges = chain_ranges(&sel);

        // The narrowest range is the identifier `y`.
        assert_eq!(ranges[0].start.line, 1);
        assert_eq!(ranges[0].start.character, 0);
        assert_eq!(ranges[0].end.character, 1);

        // The enclosing statement (the middle level) must start on
        // line 1 and cover at least the `y <- x + 1` text.
        let stmt_level = ranges
            .iter()
            .find(|r| r.start.line == 1 && r.end.character > 1)
            .unwrap_or_else(|| panic!("expected a statement-level range on line 1: {:?}", ranges));
        assert!(
            stmt_level.start.character == 0,
            "statement range should start at col 0: {:?}",
            stmt_level
        );
    }

    #[test]
    fn selection_range_no_identifier_falls_back_to_cursor() {
        // Cursor on whitespace (between `<-` and the value) is not on
        // an identifier. The narrowest range must be a zero-width
        // span at the cursor so the editor still has an anchor.
        let src = "x <- 1L\n";
        // Cursor on the space after `<-` (line 0, col 4).
        let pos = Position {
            line: 0,
            character: 4,
        };
        let sel = selection_range_at(src, pos);
        let ranges = chain_ranges(&sel);

        // The narrowest range is a zero-width span at the cursor.
        assert_eq!(ranges[0].start, pos);
        assert_eq!(ranges[0].end, pos);

        // The chain still widens to the file level.
        let widest = ranges.last().unwrap();
        assert_eq!(widest.start.line, 0);
        assert_eq!(widest.start.character, 0);
    }

    #[test]
    fn position_to_byte_offset_basic() {
        // The helper must map LSP positions back to byte offsets in
        // the source text. This is the inverse of
        // `byte_offset_to_position` for ASCII text.
        let text = "x <- 1L\ny <- 2L\n";
        // (0, 0) -> byte 0 (the 'x').
        assert_eq!(
            position_to_byte_offset(
                text,
                Position {
                    line: 0,
                    character: 0,
                }
            ),
            0
        );
        // (0, 5) -> byte 5 (the '1').
        assert_eq!(
            position_to_byte_offset(
                text,
                Position {
                    line: 0,
                    character: 5,
                }
            ),
            5
        );
        // (1, 0) -> byte 8 (the 'y', right after the first '\n').
        assert_eq!(
            position_to_byte_offset(
                text,
                Position {
                    line: 1,
                    character: 0,
                }
            ),
            8
        );
    }
}
