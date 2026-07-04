//! LSP backend: `Backend`, `State`, the `LanguageServer` impl, and the
//! document cache / debounce machinery (PLAN Phase E3 -- extracted from
//! `lib.rs` so `lib.rs` is just module declarations + `run()`).
//!
//! All request handlers read the cached parse (`State::parsed`) and the
//! cached single-file scope (`State::scopes`) instead of re-parsing /
//! re-checking on every request (PLAN Phase E1/E2). Diagnostics are
//! debounced via `schedule_diagnostics`.

use crate::diagnostics::{
    diagnostic_to_lsp, diagnostic_to_lsp_with_source, make_ignore_action, make_ignore_file_action,
};
use crate::folding::collect_folding_ranges;
use crate::hints::{collect_completions, collect_inlay_hints, find_enclosing_call, get_signature};
use crate::ident::find_ident_at_offset;
use crate::navigation::{
    collect_document_highlights, find_definition_locations, find_references_in_file,
};
use crate::selection::build_selection_range;
use crate::symbols::{collect_symbols, flatten_symbols_to_symbol_info};
use crate::util::{position_to_byte_offset_pos, span_to_range};
use ry_checker::Project;
use ry_core::{RParser, SourceFile};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::Diagnostic as LspDiagnostic;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

#[derive(Debug, Clone)]
pub(super) struct Backend {
    pub(super) client: Client,
    pub(super) state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
pub(super) struct State {
    /// Open documents: path -> current source text. Keeping every open
    /// document's text lets us rebuild a multi-file `Project` on each
    /// change so cross-file resolution (function defined in `a.R`
    /// visible from `b.R` when both are open in the editor) works.
    docs: HashMap<String, String>,
    /// path -> version of the most recent edit. `did_open`/`did_change`
    /// record the version here so cache freshness can be validated.
    versions: HashMap<String, i32>,
    /// path -> (version, parsed SourceFile). Populated lazily by the
    /// request handlers and invalidated by `update_doc`. Reading the
    /// cached parse lets every handler avoid re-parsing on each request
    /// (PLAN Phase E1). `SourceFile` is `Send`; `RParser` is NOT, so the
    /// parser is constructed per request and only the result is cached.
    parsed: HashMap<String, (i32, SourceFile)>,
    /// path -> (version, top-level Scope from `check_with_scope`).
    /// Reused by hover/inlay/completion so they don't re-run the
    /// single-file check on every request (PLAN Phase E2). Invalidated
    /// by `update_doc` alongside the parse cache.
    scopes: HashMap<String, (i32, ry_checker::Scope)>,
    /// Debounce counter per path. `schedule_diagnostics` bumps this and
    /// spawns a task that sleeps, then only publishes if its generation
    /// is still the latest (PLAN Phase E2). A newer edit during the
    /// sleep window wins and the stale task aborts.
    diag_generation: HashMap<String, u64>,
    /// Workspace root, set at `initialize`. Used only for diagnostics
    /// today; future revisions may use it to discover `ry.toml`.
    #[allow(dead_code)]
    root: Option<PathBuf>,
    /// Counts every actual parse (`RParser::parse`) performed by
    /// `parsed_file` -- i.e. every cache MISS. The E1 acceptance test
    /// asserts that editing one file in a multi-file workspace parses
    /// only that file, so this counter must NOT rise for cache hits
    /// (PLAN Phase E1).
    #[cfg(test)]
    pub(super) parse_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl State {
    /// Return the cached parse for `path` when its version matches the
    /// latest recorded version, else `None`. Pure cache read -- does
    /// NOT parse. Split out of `parsed_file` so the cache behavior is
    /// unit-testable on a bare `State` without constructing a
    /// `tower_lsp::Client` (PLAN Phase E1).
    pub(super) fn cached_parse(&self, path: &str) -> Option<SourceFile> {
        let version = self.versions.get(path).copied()?;
        let (cached_v, file) = self.parsed.get(path)?;
        if *cached_v == version {
            Some(file.clone())
        } else {
            None
        }
    }

    /// Store a freshly-parsed `SourceFile` against `version`, bumping
    /// the parse counter (test builds only). If a newer edit landed in
    /// the meantime (`versions[path] != version`), the stale parse is
    /// dropped rather than cached. Returns whether the parse was stored.
    pub(super) fn record_parse(&mut self, path: &str, version: i32, file: SourceFile) -> bool {
        #[cfg(test)]
        self.parse_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.versions.get(path).copied() == Some(version) {
            self.parsed.insert(path.to_string(), (version, file));
            true
        } else {
            false
        }
    }

    /// Snapshot of the parse counter (number of cache misses / actual
    /// parses since `State` was created). Test-only.
    #[cfg(test)]
    pub(super) fn parse_count(&self) -> usize {
        self.parse_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Drop the cached parse and scope for `path`, mirroring the
    /// cache-invalidation half of `Backend::update_doc`. Test-only;
    /// lets the E1 acceptance test simulate a `did_change` on a bare
    /// `State` without a `tower_lsp::Client`.
    #[cfg(test)]
    pub(super) fn invalidate_parse(&mut self, path: &str) {
        self.parsed.remove(path);
        self.scopes.remove(path);
    }

    /// Open / replace a document at `version`, mirroring the doc-store
    /// half of `Backend::update_doc`. Test-only.
    #[cfg(test)]
    pub(super) fn set_doc(&mut self, path: &str, text: String, version: i32) {
        self.docs.insert(path.to_string(), text);
        self.versions.insert(path.to_string(), version);
    }

    /// Read-only access to a document's source text. Test-only.
    #[cfg(test)]
    pub(super) fn doc_text(&self, path: &str) -> Option<&str> {
        self.docs.get(path).map(|s| s.as_str())
    }
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
        let version = params.text_document.version;
        self.update_doc(path, text, version).await;
        self.schedule_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let version = params.text_document.version;
        // TextDocumentSyncKind::FULL means the whole new text arrives in
        // `changes[0]`. v1 does not implement incremental sync, so we
        // ignore any further entries in `content_changes`.
        if let Some(change) = params.content_changes.into_iter().next() {
            self.update_doc(path, change.text, version).await;
            // Debounced: a burst of keystrokes coalesces into a single
            // diagnostic publish (PLAN Phase E2).
            self.schedule_diagnostics(uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        {
            let mut state = self.state.lock().await;
            state.docs.remove(&path);
            state.versions.remove(&path);
            state.parsed.remove(&path);
            state.scopes.remove(&path);
            state.diag_generation.remove(&path);
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

        // Parse (cached: PLAN Phase E1) and reuse the cached scope (PLAN
        // Phase E2) for the type lookup.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let Some(scope) = self.scope_for(&path).await else {
            return Ok(None);
        };

        // Find the identifier at the hover position via an AST walk
        // (smallest enclosing Expr::Ident), so non-ASCII identifiers and
        // identifiers in any syntactic position resolve correctly.
        let byte_offset = position_to_byte_offset_pos(&text, position);
        let Some((identifier, _)) = find_ident_at_offset(&file, byte_offset) else {
            return Ok(None);
        };

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

        // Parse the current document (cached: PLAN Phase E1). We do not
        // need the checker's scope here: definitions live in the AST,
        // not the type environment.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        // Find the identifier under the cursor via an AST walk. Returns
        // `None` (no definition) for operators, numbers, and keywords.
        let byte_offset = position_to_byte_offset_pos(&text, position);
        let Some((identifier, _)) = find_ident_at_offset(&file, byte_offset) else {
            return Ok(None);
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

        // Find the identifier under the cursor via an AST walk of the
        // current document (cached: PLAN Phase E1). Returns `None` for
        // operators, numbers, and keywords.
        let Some(current_file) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let byte_offset = position_to_byte_offset_pos(&text, position);
        let Some((identifier, _)) = find_ident_at_offset(&current_file, byte_offset) else {
            return Ok(None);
        };

        let mut all_locations = Vec::new();
        for (doc_path, doc_text) in &docs {
            let file = if doc_path == &path {
                current_file.clone()
            } else {
                // Use the cached parse; skip documents that fail to parse
                // rather than aborting the whole search.
                match self.parsed_file(doc_path).await {
                    Some(f) => f,
                    None => continue,
                }
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

        // Reuse the cached parse (PLAN Phase E1) and cached single-file
        // scope (PLAN Phase E2) so the symbol panel doesn't re-check on
        // every request. Symbols nested inside function bodies fall back
        // to "function" / "variable" since the top-level scope does not
        // track locals.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let Some(scope) = self.scope_for(&path).await else {
            return Ok(None);
        };

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

        // Parse the document (cached: PLAN Phase E1). On any parse
        // failure we return `None` (no hints) rather than erroring, so
        // the editor simply shows nothing instead of a broken state.
        // Mirrors `document_symbol`.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        // Reuse the cached single-file scope (PLAN Phase E2) for the
        // inferred type annotations.
        let Some(scope) = self.scope_for(&path).await else {
            return Ok(None);
        };

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

        // Reuse the cached scope (PLAN Phase E2), which parses lazily
        // via the parse cache. Mirrors `hover` and `inlay_hint`: on any
        // parse failure we return `None` (no completions).
        let Some(scope) = self.scope_for(&path).await else {
            return Ok(None);
        };

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

        let mut all_symbols: Vec<SymbolInformation> = Vec::new();
        for (doc_path, doc_text) in &docs {
            // Cached parse (PLAN Phase E1) and cached single-file scope
            // (PLAN Phase E2); skip documents that fail to parse rather
            // than aborting the whole search.
            let Some(file) = self.parsed_file(doc_path).await else {
                continue;
            };
            let Some(scope) = self.scope_for(doc_path).await else {
                continue;
            };

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

        // Find the identifier at the cursor position via an AST walk to
        // learn the old name (cached parse: PLAN Phase E1). We rename
        // ALL occurrences of that name across all open documents,
        // mirroring how `references` works. Returns `None` (no rename)
        // for operators, numbers, keywords.
        let Some(current_file) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let byte_offset = position_to_byte_offset_pos(&text, position);
        let Some((old_name, _)) = find_ident_at_offset(&current_file, byte_offset) else {
            return Ok(None);
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
            let file = if doc_path == &path {
                current_file.clone()
            } else {
                match self.parsed_file(doc_path).await {
                    Some(f) => f,
                    None => continue,
                }
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

        // Validate that the cursor is on a renameable identifier before
        // the editor shows the rename UI. Use the AST-based finder so we
        // get the exact span of the innermost identifier, then convert
        // it to an LSP range. Returns `None` for operators, numbers,
        // keywords, and whitespace.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let byte_offset = position_to_byte_offset_pos(&text, position);
        let Some((_, span)) = find_ident_at_offset(&file, byte_offset) else {
            return Ok(None);
        };
        let range = span_to_range(&text, span).unwrap_or(Range {
            start: position,
            end: Position {
                line: position.line,
                character: position.character + 1,
            },
        });

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

        // Parse the current document (cached: PLAN Phase E1). Document
        // highlight is scoped to the current file (per the LSP spec), so
        // we only parse once.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        // Find the identifier under the cursor via an AST walk. Returns
        // `None` (no highlights) for operators, numbers, and keywords.
        let byte_offset = position_to_byte_offset_pos(&text, position);
        let Some((identifier, _)) = find_ident_at_offset(&file, byte_offset) else {
            return Ok(None);
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

        // Parse the document (cached: PLAN Phase E1). On any parse
        // failure we return `None` (no folding ranges) rather than
        // erroring. Mirrors `document_symbol` / `inlay_hint`.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
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

        // Parse the document (cached: PLAN Phase E1). On any parse
        // failure we return `None` (no selection ranges) rather than
        // erroring. Mirrors `document_symbol` / `folding_range`.
        let Some(file) = self.parsed_file(&path).await else {
            return Ok(None);
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
    async fn update_doc(&self, path: String, text: String, version: i32) {
        let mut state = self.state.lock().await;
        state.docs.insert(path.clone(), text);
        state.versions.insert(path.clone(), version);
        // Invalidate the cached parse and scope; the next read
        // repopulates them (PLAN Phase E1/E2).
        state.parsed.remove(&path);
        state.scopes.remove(&path);
    }

    /// Return the parsed `SourceFile` for `path`, reusing the cached
    /// parse when its version matches the latest known version. The
    /// cache is read + repopulated under the state lock; parsing itself
    /// (which needs a non-`Send` `RParser`) happens after releasing the
    /// lock, then the result is stored back (PLAN Phase E1).
    ///
    /// Returns `None` when the path is not an open document or parsing
    /// fails.
    async fn parsed_file(&self, path: &str) -> Option<SourceFile> {
        // Fast path: cache hit with matching version.
        {
            let state = self.state.lock().await;
            if let Some(file) = state.cached_parse(path) {
                return Some(file);
            }
        }
        // Cache miss / stale: parse the current text and store it.
        let (text, version) = {
            let state = self.state.lock().await;
            (
                state.docs.get(path).cloned(),
                state.versions.get(path).copied(),
            )
        };
        let (text, version) = match (text, version) {
            (Some(t), Some(v)) => (t, v),
            _ => return None,
        };
        let mut parser = RParser::new().ok()?;
        let file = parser.parse(path, &text).ok()?;
        let mut state = self.state.lock().await;
        // Re-validate version under the lock: a concurrent edit could
        // have invalidated what we just parsed.
        state.record_parse(path, version, file.clone());
        Some(file)
    }

    /// Return the top-level `Scope` for `path`, reusing the cached
    /// single-file `check_with_scope` result when its version matches.
    /// Used by hover/inlay/completion so they don't re-run the check on
    /// every request (PLAN Phase E2). Returns `None` when the document
    /// is not open or parsing fails.
    async fn scope_for(&self, path: &str) -> Option<ry_checker::Scope> {
        // Fast path: cached scope with matching version.
        {
            let state = self.state.lock().await;
            if let Some(version) = state.versions.get(path).copied() {
                if let Some((cached_v, scope)) = state.scopes.get(path) {
                    if *cached_v == version {
                        return Some(scope.clone());
                    }
                }
            }
        }
        // Cache miss: parse (via the parse cache) + check, then store.
        let file = self.parsed_file(path).await?;
        let version = {
            let state = self.state.lock().await;
            state.versions.get(path).copied()
        };
        let mut checker = ry_checker::Checker::new(path);
        let (_, scope) = checker.check_with_scope(&file);
        let mut state = self.state.lock().await;
        if let Some(version) = version {
            if state.versions.get(path).copied() == Some(version) {
                state
                    .scopes
                    .insert(path.to_string(), (version, scope.clone()));
            }
        }
        Some(scope)
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
        let mut per_file_comments: HashMap<String, Vec<ry_core::ast::Comment>> = HashMap::new();
        // Cached parses (PLAN Phase E1): `parsed_file` reuses the
        // per-document `SourceFile` cached in `State` and only re-parses
        // documents whose version changed since the last request.
        for doc_path in docs.keys() {
            let Some(file) = self.parsed_file(doc_path).await else {
                continue;
            };
            per_file_comments.insert(doc_path.clone(), file.comments.clone());
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
        // per-diagnostic filter below is a cheap lookup. Use the lexical
        // (comment-based) parsers so a `#` inside a string literal is
        // not mistaken for a directive.
        let file_comments = per_file_comments.get(path.as_str());
        let (file_level, suppressions) = match file_comments {
            Some(cs) => (
                ry_checker::has_file_suppression_from_comments(cs),
                ry_checker::parse_suppressions_from_comments(cs),
            ),
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

    /// Debounce diagnostics for `uri`: bump a per-path generation counter
    /// and spawn a task that sleeps ~180ms, then publishes diagnostics
    /// ONLY if its generation is still the latest. A newer edit during
    /// the sleep window bumps the counter and the stale task aborts, so
    /// a burst of keystrokes triggers a single check rather than one per
    /// keystroke (PLAN Phase E2).
    async fn schedule_diagnostics(&self, uri: Url) {
        let path = uri_to_path(&uri);
        // Bump the generation under the lock; capture the value the
        // spawned task must match to publish.
        let generation = {
            let mut state = self.state.lock().await;
            let g = state
                .diag_generation
                .get(&path)
                .copied()
                .unwrap_or(0)
                .wrapping_add(1);
            state.diag_generation.insert(path, g);
            g
        };
        let backend = Backend {
            client: self.client.clone(),
            state: Arc::clone(&self.state),
        };
        let watch_path = uri_to_path(&uri);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            // Only publish if no newer edit arrived during the sleep.
            let stale = {
                let state = backend.state.lock().await;
                state.diag_generation.get(&watch_path).copied() != Some(generation)
            };
            if !stale {
                backend.publish_diagnostics(uri).await;
            }
        });
    }
}

/// Convert a document's path string (the key used in `State::docs`)
/// back into an LSP `Url`. Filesystem paths round-trip via
/// `Url::from_file_path`; non-file URIs (e.g. `untitled:`) fall back to
/// `Url::parse`.
pub(crate) fn path_to_uri(path: &str) -> Url {
    Url::from_file_path(path).unwrap_or_else(|_| {
        Url::parse(path).unwrap_or_else(|_| Url::parse("file:///unknown").unwrap())
    })
}

/// Convert a `file://` URI to a filesystem path string. Falls back to
/// the URI's string form when the URI isn't a `file:` scheme (so a
/// virtual or untitled document still gets a stable key).
pub(crate) fn uri_to_path(uri: &Url) -> String {
    uri.to_file_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| uri.as_str().to_string())
}
