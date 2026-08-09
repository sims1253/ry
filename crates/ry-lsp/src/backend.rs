//! LSP backend: `Backend`, `State`, the `LanguageServer` impl, and the
//! document cache / debounce machinery (extracted from
//! `lib.rs` so `lib.rs` is just module declarations + `run()`).
//!
//! All request handlers read the cached parse (`State::parsed`) and the
//! cached single-file scope (`State::scopes`) instead of re-parsing /
//! re-checking on every request. Diagnostics are
//! debounced via `schedule_diagnostics`.

use crate::diagnostics::{
    diagnostic_to_lsp, diagnostic_to_lsp_with_source, make_ignore_action, make_ignore_file_action,
};
use crate::folding::collect_folding_ranges;
use crate::hints::{
    active_parameter, collect_completions, collect_inlay_hints, find_enclosing_call, get_signature,
};
use crate::ident::{find_ident_at_offset, is_valid_identifier};
use crate::navigation::{
    collect_document_highlights, find_definition_locations, find_references_in_file,
};
use crate::selection::build_selection_range;
use crate::settings::{FolderSettings, ServerSettings};
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

#[derive(Clone)]
pub(super) struct Backend {
    pub(super) client: Client,
    pub(super) state: Arc<Mutex<State>>,
}

#[derive(Default)]
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
    ///. `SourceFile` is `Send`; `RParser` is NOT, so the
    /// parser is constructed per request and only the result is cached.
    parsed: HashMap<String, (i32, Arc<SourceFile>)>,
    /// path -> (version, top-level Scope from `check_with_scope`).
    /// Reused by hover/inlay/completion so they don't re-run the
    /// single-file check on every request. Invalidated
    /// by `update_doc` alongside the parse cache.
    scopes: HashMap<String, (i32, ry_checker::Scope)>,
    /// Workspace-wide debounce counter. `schedule_diagnostics` bumps this and
    /// spawns a task that sleeps, then only publishes if its generation
    /// is still the latest. A newer edit during the
    /// sleep window wins and the stale task aborts.
    diag_generation: u64,
    /// Runtime stubs loaded from the workspace's `ry.toml`. Kept in state so
    /// every rebuilt Project and single-file hover checker sees the same data.
    user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Persistent multi-file checker used only by diagnostics. Its own mutex
    /// keeps project checks serialized without holding the document-state
    /// lock used by latency-sensitive LSP requests.
    project: Arc<Mutex<ProjectCache>>,
    /// Counts every actual parse (`RParser::parse`) performed by
    /// `parsed_file` -- i.e. every cache MISS. The E1 acceptance test
    /// asserts that editing one file in a multi-file workspace parses
    /// only that file, so this counter must NOT rise for cache hits
    ///.
    #[cfg(test)]
    pub(super) parse_count: Arc<std::sync::atomic::AtomicUsize>,

    // --- S2: settings channel ---
    /// The workspace root directory (from `root_uri`), used for
    /// `ry.toml` discovery and relative path resolution.
    root: Option<PathBuf>,
    /// The full `ry-config::Config` loaded from `ry.toml` at the
    /// workspace root. Stored so the severity filter and baseline can
    /// be applied in `publish_diagnostics` without re-reading the file.
    file_config: ry_config::Config,
    /// Editor-supplied per-folder settings, received via
    /// `initializationOptions`, `workspace/configuration`, or
    /// `didChangeConfiguration`.
    folder_settings: FolderSettings,
    /// Whether the client supports the `workspace/configuration`
    /// pull request. When true, `didChangeConfiguration` re-pulls
    /// instead of parsing the notification payload.
    supports_workspace_configuration: bool,
    /// Whether the client supports dynamic registration of
    /// `workspace/didChangeWatchedFiles` (S3).
    supports_did_change_watched_files: bool,

    // --- S4: multi-root workspace folders ---
    /// Workspace folder roots, each with its own loaded `ry.toml` config.
    /// Empty when only a single `root_uri` is provided. Each entry is
    /// (folder_root, file_config), ordered by root path length descending
    /// so that longest-prefix matching finds the most specific folder first.
    workspace_folders: Vec<(PathBuf, ry_config::Config)>,
    /// Filesystem-derived resolution state, ordered by longest root prefix.
    workspace_contexts: Vec<(PathBuf, ry_workspace::WorkspaceContext)>,
    /// On-disk `.R`/`.r` files discovered by the background indexer
    /// (Plan 33 W4). Keyed by absolute path. Open documents shadow
    /// these — when a path exists in both `docs` and `disk_files`,
    /// the open document's content is authoritative.
    disk_files: HashMap<String, Arc<SourceFile>>,
    /// Tree-sitter trees for incremental parsing (Plan 33 W6).
    trees: HashMap<String, ry_core::Tree>,
}

#[derive(Default)]
pub(super) struct ProjectCache {
    project: Project,
    /// Snapshot identity for every file currently installed in `project`.
    /// The LSP version is sufficient for open documents, but indexed files
    /// use version zero, so `Arc` identity also participates in freshness.
    files: HashMap<String, (i32, Arc<SourceFile>)>,
}

pub(super) struct ProjectCheckResult {
    diagnostics: Vec<(String, Vec<ry_checker::Diagnostic>)>,
    /// The exact parsed snapshots supplied to this check. Publication uses
    /// their owned source and comments, never a separately-read document.
    files: HashMap<String, Arc<SourceFile>>,
}

impl ProjectCache {
    #[cfg(test)]
    pub(super) fn check(
        &mut self,
        files: Vec<(String, i32, Arc<SourceFile>)>,
        user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    ) -> Vec<(String, Vec<ry_checker::Diagnostic>)> {
        self.check_with_workspace(files, user_stubs, None)
            .diagnostics
    }

    pub(super) fn check_with_workspace(
        &mut self,
        files: Vec<(String, i32, Arc<SourceFile>)>,
        user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
        workspace: Option<&ry_workspace::WorkspaceContext>,
    ) -> ProjectCheckResult {
        let checked_files = files
            .iter()
            .map(|(path, _, file)| (path.clone(), Arc::clone(file)))
            .collect();
        let current_paths: std::collections::HashSet<&str> =
            files.iter().map(|(path, _, _)| path.as_str()).collect();
        let removed: Vec<String> = self
            .files
            .keys()
            .filter(|path| !current_paths.contains(path.as_str()))
            .cloned()
            .collect();
        for path in removed {
            self.project.remove_file(&path);
            self.files.remove(&path);
        }

        self.project.set_user_stubs(user_stubs);
        let empty_workspace = ry_workspace::WorkspaceContext::default();
        let workspace = workspace.unwrap_or(&empty_workspace);
        self.project.set_loaded(workspace.attached_packages.clone());
        self.project
            .set_bare_loaded(workspace.bare_bindings.clone());
        self.project
            .set_external_bindings(workspace.external_bindings.clone());
        self.project
            .set_imported_from(workspace.imported_bindings.clone());
        self.project
            .set_external_s3_methods(workspace.s3_methods.clone());
        self.project
            .set_load_bindings(workspace.load_bindings.clone());
        for (path, version, file) in files {
            let changed = self
                .files
                .get(&path)
                .is_none_or(|(cached_version, cached)| {
                    *cached_version != version || !Arc::ptr_eq(cached, &file)
                });
            if changed {
                self.project.update_file(path.clone(), Arc::clone(&file));
                self.files.insert(path, (version, file));
            }
        }
        ProjectCheckResult {
            diagnostics: self.project.check_incremental(),
            files: checked_files,
        }
    }
}

impl State {
    /// Return the cached parse for `path` when its version matches the
    /// latest recorded version, else `None`. Pure cache read -- does
    /// NOT parse. Split out of `parsed_file` so the cache behavior is
    /// unit-testable on a bare `State` without constructing a
    /// `tower_lsp::Client`.
    pub(super) fn cached_parse(&self, path: &str) -> Option<Arc<SourceFile>> {
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
    pub(super) fn record_parse(&mut self, path: &str, version: i32, file: Arc<SourceFile>) -> bool {
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

    /// Mutable access to editor settings for tests.
    #[cfg(test)]
    pub(super) fn folder_settings_mut(&mut self) -> &mut crate::settings::FolderSettings {
        &mut self.folder_settings
    }

    /// Mutable access to file config for tests.
    #[cfg(test)]
    pub(super) fn file_config_mut(&mut self) -> &mut ry_config::Config {
        &mut self.file_config
    }

    // --- S2: effective config computation ---

    /// Compute the effective `SeverityFilter` from editor settings
    /// merged over `ry.toml`. Editor settings take precedence: if the
    /// editor provides an `ignore`/`error`/`warn` list, it replaces
    /// the `ry.toml` value; otherwise the `ry.toml` value (which may
    /// itself be the empty default) is used.
    /// Merge editor lint settings over a given `ry.toml` config.
    pub(super) fn merge_filter(
        &self,
        file_config: &ry_config::Config,
    ) -> ry_checker::SeverityFilter {
        let lint = &self.folder_settings.lint;
        let error = lint
            .error
            .clone()
            .unwrap_or_else(|| file_config.error.clone());
        let warn = lint
            .warn
            .clone()
            .unwrap_or_else(|| file_config.warn.clone());
        let ignore = lint
            .ignore
            .clone()
            .unwrap_or_else(|| file_config.ignore.clone());
        let mut filter = ry_config::build_filter(&error, &warn, &ignore);
        let select = lint.select.as_ref().or(file_config.select.as_ref());
        let extend_select = lint
            .extend_select
            .as_ref()
            .unwrap_or(&file_config.extend_select);
        if let Some(select) = select {
            filter.begin_selection();
            for rule in select {
                filter.add_select(rule);
            }
        }
        for rule in extend_select {
            filter.add_extend_select(rule);
        }
        filter
    }

    /// Test helper: compute the effective `SeverityFilter` from editor
    /// settings merged over the root `ry.toml`.
    #[cfg(test)]
    pub(super) fn effective_filter(&self) -> ry_checker::SeverityFilter {
        self.merge_filter(&self.file_config)
    }

    /// S4: Find the owning workspace folder config for a document path.
    /// Uses longest-prefix matching against workspace folder roots.
    /// Returns None if no workspace folder owns the path.
    pub(super) fn folder_config_for_path(&self, doc_path: &str) -> Option<&ry_config::Config> {
        let path = std::path::Path::new(doc_path);
        for (folder_root, config) in &self.workspace_folders {
            if path.starts_with(folder_root) {
                return Some(config);
            }
        }
        None
    }

    fn workspace_context_for_path(
        &self,
        doc_path: &str,
    ) -> Option<&ry_workspace::WorkspaceContext> {
        let path = std::path::Path::new(doc_path);
        self.workspace_contexts
            .iter()
            .find_map(|(root, context)| path.starts_with(root).then_some(context))
    }

    fn eligibility_for_path(&self, doc_path: &str) -> bool {
        let path = std::path::Path::new(doc_path);
        if let Some((root, config)) = self
            .workspace_folders
            .iter()
            .find(|(root, _)| path.starts_with(root))
        {
            return ry_workspace::is_file_eligible(path, root, config);
        }
        match &self.root {
            Some(root) => ry_workspace::is_file_eligible(path, root, &self.file_config),
            None => true,
        }
    }

    /// Load and return the effective baseline, if any. Editor setting
    /// takes precedence over `ry.toml`.
    pub(super) fn effective_baseline(&self) -> Option<ry_config::Baseline> {
        let baseline_path = self
            .folder_settings
            .baseline
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| self.file_config.baseline.clone())?;
        let resolved = if baseline_path.is_relative() {
            self.root.as_deref().map(|r| r.join(&baseline_path))?
        } else {
            baseline_path
        };
        ry_config::load_baseline(&resolved).ok()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let root = params.root_uri.and_then(|uri| uri.to_file_path().ok());

        // S2: Read initializationOptions if present. This is the only
        // settings channel Zed can drive, so it must be sufficient on
        // its own. The shape mirrors ruff-vscode's: an array of
        // per-folder settings plus a global fallback.
        let server_settings: ServerSettings = params
            .initialization_options
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Use the first folder's settings, falling back to the global
        // settings. (Multi-root is S4; for now we take the first entry.)
        let folder_settings = server_settings
            .settings
            .into_iter()
            .next()
            .unwrap_or(server_settings.global_settings);

        // Check if the client supports workspace/configuration pull.
        let supports_workspace_configuration = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.configuration)
            .unwrap_or(false);

        let supports_did_change_watched_files = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.did_change_watched_files.as_ref())
            .and_then(|f| f.dynamic_registration)
            .unwrap_or(false);

        // `load_workspace_stubs` does blocking filesystem I/O (reads
        // `ry.toml` and walks typeshed directories); run it off the async
        // executor so a slow disk does not stall every LSP request.
        // We also load the full `ry-config::Config` here so the severity
        // filter and baseline are available in `publish_diagnostics`.
        let root_clone = root.clone();
        let user_stubs =
            tokio::task::spawn_blocking(move || load_workspace_stubs(root_clone.as_deref()))
                .await
                .unwrap_or_else(|_| Arc::new(std::collections::BTreeMap::new()));

        // Load the full ry.toml config for severity filtering (S2).
        let file_config = root
            .as_deref()
            .and_then(|r| ry_config::Config::load_from_dir(r).ok().flatten())
            .unwrap_or_default();

        // S4: Load per-folder configs for multi-root workspaces.
        let mut ws_folders: Vec<(PathBuf, ry_config::Config)> = params
            .workspace_folders
            .as_ref()
            .map(|folders| {
                folders
                    .iter()
                    .filter_map(|f| f.uri.to_file_path().ok())
                    .map(|path| {
                        let cfg = ry_config::Config::load_from_dir(&path)
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        (path, cfg)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut state = self.state.lock().await;
        state.user_stubs = user_stubs;
        state.root = root;
        state.file_config = file_config;
        state.folder_settings = folder_settings;
        state.supports_workspace_configuration = supports_workspace_configuration;
        state.supports_did_change_watched_files = supports_did_change_watched_files;
        // Longest-prefix matching in `folder_config_for_path` requires
        // the most specific root first.
        ws_folders.sort_by_key(|(p, _)| std::cmp::Reverse(p.as_os_str().len()));
        state.workspace_folders = ws_folders;
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // All position conversion in this server is UTF-16. Advertise
                // it explicitly instead of relying on the protocol default.
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    // Incremental sync (Plan 33 W6): the client sends
                    // only the edited range, which we use to build a
                    // tree-sitter InputEdit for incremental reparse.
                    TextDocumentSyncKind::INCREMENTAL,
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
                // S4: Advertise workspace folder support so clients send
                // multi-root workspace folders and change notifications.
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
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

        // S2: If the client supports workspace/configuration, pull the
        // `ry.*` section now. This is the primary settings path for VS
        // Code and supersedes whatever was in initializationOptions.
        let should_pull = {
            let state = self.state.lock().await;
            state.supports_workspace_configuration
        };
        if should_pull {
            let root_uri = {
                let state = self.state.lock().await;
                state
                    .root
                    .as_ref()
                    .and_then(|p| Url::from_file_path(p).ok())
            };
            let item = ConfigurationItem {
                scope_uri: root_uri,
                section: Some("ry".to_string()),
            };
            match self.client.configuration(vec![item]).await {
                Ok(values) => {
                    if let Some(value) = values.into_iter().next() {
                        if let Ok(settings) = serde_json::from_value::<FolderSettings>(value) {
                            let mut state = self.state.lock().await;
                            state.folder_settings = settings;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("workspace/configuration pull failed: {e}");
                }
            }
        }

        // Register workspace-resolution watchers so configuration, package
        // metadata, serialized data, and local stubs refresh without restart.
        // Gated on the client's dynamic-registration capability.
        let supports_watchers = {
            // Read from the stored capabilities (set in initialize).
            // We check the workspace.didChangeWatchedFiles capability.
            let state = self.state.lock().await;
            state.supports_did_change_watched_files
        };
        if supports_watchers {
            let watcher_registration = Registration {
                id: "ry-workspace-watcher".to_string(),
                method: "workspace/didChangeWatchedFiles".into(),
                register_options: Some(serde_json::json!({
                    "watchers": [
                        {"globPattern": "**/ry.toml"},
                        {"globPattern": "**/DESCRIPTION"},
                        {"globPattern": "**/NAMESPACE"},
                        {"globPattern": "**/*.{rda,RData,rdata,json}"}
                    ]
                })),
            };
            if let Err(e) = self
                .client
                .register_capability(vec![watcher_registration])
                .await
            {
                tracing::warn!("failed to register workspace watcher: {e}");
            }
        }

        // W4: Spawn a background indexer to discover and parse all .R/.r
        // files under the workspace root(s).
        self.spawn_background_index().await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let text = params.text_document.text.clone();
        let version = params.text_document.version;
        // Clear any stale tree from a previous session for this path.
        {
            let mut state = self.state.lock().await;
            state.trees.remove(&path);
        }
        self.update_doc(path, text, version).await;
        self.schedule_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let version = params.text_document.version;
        // Incremental sync (W6): process each change event, applying
        // range-based edits to the old text and building a tree-sitter
        // InputEdit for incremental reparse.
        for change in params.content_changes {
            self.apply_incremental_change(&path, change, version).await;
        }
        // Debounced: a burst of keystrokes coalesces into a single
        // diagnostic publish.
        self.schedule_diagnostics(uri).await;
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // S2: Configuration changed. If the client supports pull
        // configuration, re-pull the `ry.*` section. Otherwise parse
        // the settings blob sent in the notification.
        let should_pull = {
            let state = self.state.lock().await;
            state.supports_workspace_configuration
        };

        if should_pull {
            let root_uri = {
                let state = self.state.lock().await;
                state
                    .root
                    .as_ref()
                    .and_then(|p| Url::from_file_path(p).ok())
            };
            let item = ConfigurationItem {
                scope_uri: root_uri,
                section: Some("ry".to_string()),
            };
            if let Ok(values) = self.client.configuration(vec![item]).await {
                if let Some(value) = values.into_iter().next() {
                    if let Ok(settings) = serde_json::from_value::<FolderSettings>(value) {
                        let mut state = self.state.lock().await;
                        state.folder_settings = settings;
                    }
                }
            }
        } else {
            // The client sent settings inline. They may be wrapped in
            // an outer "ry" key (VS Code) or be the raw ry settings.
            let raw = &params.settings;
            let ry_section = raw.get("ry").unwrap_or(raw);
            if let Ok(settings) = serde_json::from_value::<FolderSettings>(ry_section.clone()) {
                let mut state = self.state.lock().await;
                state.folder_settings = settings;
            }
        }

        self.spawn_background_index().await;

        // Republish diagnostics for every open document so the new
        // settings take effect immediately.
        let open_uris: Vec<Url> = {
            let state = self.state.lock().await;
            state.docs.keys().map(|p| path_to_uri(p)).collect()
        };
        for uri in open_uris {
            self.schedule_diagnostics(uri).await;
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        // S4: Handle workspace folder additions and removals.
        let mut added_configs: Vec<(PathBuf, ry_config::Config)> = Vec::new();
        for folder in &params.event.added {
            if let Ok(path) = folder.uri.to_file_path() {
                let cfg = ry_config::Config::load_from_dir(&path)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                added_configs.push((path, cfg));
            }
        }

        let removed_uris: Vec<Url> = params.event.removed.iter().map(|f| f.uri.clone()).collect();

        {
            let mut state = self.state.lock().await;
            // Add new folders.
            state.workspace_folders.extend(added_configs);
            // Remove deleted folders.
            state.workspace_folders.retain(|(path, _)| {
                !removed_uris
                    .iter()
                    .any(|uri| uri.to_file_path().ok().as_deref() == Some(path))
            });
            // Sort by path length descending for longest-prefix matching.
            state
                .workspace_folders
                .sort_by_key(|(path, _)| std::cmp::Reverse(path.as_os_str().len()));
        }

        self.spawn_background_index().await;

        // Republish diagnostics for all open documents.
        let open_uris: Vec<Url> = {
            let state = self.state.lock().await;
            state.docs.keys().map(|p| path_to_uri(p)).collect()
        };
        for uri in open_uris {
            self.schedule_diagnostics(uri).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        // Refresh configuration and filesystem-backed resolution when any
        // registered package metadata, data, stub, or config file changes.
        let config_changed = params
            .changes
            .iter()
            .any(|change| change.uri.path().ends_with("ry.toml"));
        let resolution_changed = params.changes.iter().any(|change| {
            let path = change.uri.path();
            path.ends_with("DESCRIPTION")
                || path.ends_with("NAMESPACE")
                || path.ends_with("ry.toml")
                || path.ends_with(".rda")
                || path.ends_with(".RData")
                || path.ends_with(".rdata")
                || path.ends_with(".json")
        });
        if !resolution_changed {
            return;
        }

        // Reload each changed root independently. The state vectors are kept
        // in longest-prefix order, so replacing a config does not alter routing.
        let root = {
            let state = self.state.lock().await;
            state.root.clone()
        };
        if config_changed {
            for directory in params.changes.iter().filter_map(|change| {
                change
                    .uri
                    .path()
                    .ends_with("ry.toml")
                    .then(|| change.uri.to_file_path().ok())
                    .flatten()
                    .and_then(|path| path.parent().map(PathBuf::from))
            }) {
                let loaded = ry_config::Config::load_from_dir(&directory);
                match loaded {
                    Ok(config) => {
                        let config = config.unwrap_or_default();
                        let mut state = self.state.lock().await;
                        if state.root.as_deref() == Some(directory.as_path()) {
                            state.file_config = config.clone();
                        }
                        if let Some((_, folder_config)) = state
                            .workspace_folders
                            .iter_mut()
                            .find(|(folder_root, _)| folder_root == &directory)
                        {
                            *folder_config = config;
                        }
                        tracing::info!(root = %directory.display(), "workspace config reloaded");
                    }
                    Err(error) => tracing::warn!(
                        root = %directory.display(),
                        %error,
                        "failed to reload workspace config; retaining previous config"
                    ),
                }
            }
        }

        // Typeshed and configuration changes alter both package resolution
        // and the distinct user-stub analysis channel.
        if let Some(root) = root.clone()
            && let Ok(stubs) =
                tokio::task::spawn_blocking(move || load_workspace_stubs(Some(&root))).await
        {
            let mut state = self.state.lock().await;
            state.user_stubs = stubs;
        }
        self.spawn_background_index().await;

        // Republish diagnostics for every open document so the new
        // config takes effect immediately.
        let open_uris: Vec<Url> = {
            let state = self.state.lock().await;
            state.docs.keys().map(|p| path_to_uri(p)).collect()
        };
        for uri in open_uris {
            self.schedule_diagnostics(uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let remaining_open_paths = {
            let mut state = self.state.lock().await;
            state.docs.remove(&path);
            state.versions.remove(&path);
            state.parsed.remove(&path);
            state.scopes.remove(&path);
            state.trees.remove(&path);
            // Bump the generation so any in-flight diagnostics publish
            // queued while the file was open is invalidated.
            state.diag_generation = state.diag_generation.wrapping_add(1);
            state.docs.keys().cloned().collect::<Vec<_>>()
        };
        {
            let project = {
                let state = self.state.lock().await;
                Arc::clone(&state.project)
            };
            let mut project = project.lock().await;
            project.project.remove_file(&path);
            project.files.remove(&path);
        }
        // Clear diagnostics for the closed document so stale squiggles
        // don't linger after the user closes the file.
        self.client
            .publish_diagnostics(uri.clone(), Vec::new(), None)
            .await;
        // Closing a document can change diagnostics in the REMAINING open
        // documents (a name that was defined in the closed file may now
        // be unresolved, or a locally suppressed diagnostic may surface),
        // so schedule a re-publish to refresh them rather than leaving
        // stale cross-file diagnostics.
        if let Some(first) = remaining_open_paths.first() {
            self.schedule_diagnostics(path_to_uri(first)).await;
        }
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

        // Parse (cached) and reuse the cached scope for the type lookup.
        let Some((file, text)) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let Some(scope) = self.scope_for(&path).await else {
            return Ok(None);
        };

        // Find the identifier at the hover position via an AST walk
        // (smallest enclosing Expr::Ident), so non-ASCII identifiers and
        // identifiers in any syntactic position resolve correctly.
        let Some(byte_offset) = position_to_byte_offset_pos(&text, position) else {
            return Ok(None);
        };
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

        // Parse the current document (cached). We do not
        // need the checker's scope here: definitions live in the AST,
        // not the type environment.
        let Some((file, text)) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        // Find the identifier under the cursor via an AST walk. Returns
        // `None` (no definition) for operators, numbers, and keywords.
        let Some(byte_offset) = position_to_byte_offset_pos(&text, position) else {
            return Ok(None);
        };
        let Some((identifier, _)) = find_ident_at_offset(&file, byte_offset) else {
            return Ok(None);
        };

        let locations = find_definition_locations(&file, &identifier, &uri, &text);
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

        // Find the identifier under the cursor via an AST walk of the
        // current document (cached). Returns `None` for
        // operators, numbers, and keywords.
        let Some((current_file, text)) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let Some(byte_offset) = position_to_byte_offset_pos(&text, position) else {
            return Ok(None);
        };
        let Some((identifier, _)) = find_ident_at_offset(&current_file, byte_offset) else {
            return Ok(None);
        };

        let mut all_locations = Vec::new();
        for doc_path in docs.keys() {
            // Use the cache-consistent pairing: `parsed_file` returns the
            // AST and the text it was parsed FROM, so byte-offset ranges
            // generated against `doc_text` always match the AST. Skip
            // documents that fail to parse rather than aborting the search.
            let Some((file, doc_text)) = self.parsed_file(doc_path).await else {
                continue;
            };
            let doc_uri = path_to_uri(doc_path);
            let locs = find_references_in_file(
                &file,
                &identifier,
                &doc_uri,
                &doc_text,
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

        // Reuse the cached parse and cached single-file
        // scope so the symbol panel doesn't re-check on
        // every request. Symbols nested inside function bodies fall back
        // to "function" / "variable" since the top-level scope does not
        // track locals.
        let Some((file, text)) = self.parsed_file(&path).await else {
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

        // Parse the document (cached). On any parse
        // failure we return `None` (no hints) rather than erroring, so
        // the editor simply shows nothing instead of a broken state.
        // Mirrors `document_symbol`.
        let Some((file, text)) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        // Reuse the cached single-file scope for the
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

        // Reuse the cached scope, which parses lazily
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

        // Look up the function's parameter names from the base
        // typeshed. User-defined functions would require reaching
        // into the checker's FnTable from the LSP crate, which is
        // out of scope for v1.
        let Some(params_list) = get_signature(&func_name) else {
            return Ok(None);
        };

        // Build the signature label like `round(x, digits)` and the
        // per-parameter `ParameterInformation` list. Extra arguments keep
        // the final variadic parameter active; non-variadic signatures clear
        // the highlight once the cursor moves past their final parameter.
        let active_param = active_parameter(&params_list, active_param);
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
        for doc_path in docs.keys() {
            // Cached parse and cached single-file scope
            //; skip documents that fail to parse rather
            // than aborting the whole search.
            let Some((file, doc_text)) = self.parsed_file(doc_path).await else {
                continue;
            };
            let Some(scope) = self.scope_for(doc_path).await else {
                continue;
            };

            let doc_symbols = collect_symbols(&file.stmts, &doc_text, Some(&scope));
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

        // Rename inserts the supplied text without backticks, so reject names
        // that are not syntactically valid unquoted R identifiers.
        if !is_valid_identifier(&new_name) {
            return Ok(None);
        }

        // Snapshot ALL open document paths under the lock, then drop the
        // lock before parsing/walking so a slow rename doesn't block
        // other LSP requests. Rename is workspace-wide, so we walk
        // every open document (not just the current one). The per-file
        // source text comes from `parsed_file` so it always matches the
        // AST it was parsed from.
        let docs = {
            let state = self.state.lock().await;
            state.docs.keys().cloned().collect::<Vec<_>>()
        };

        // Find the identifier at the cursor position via an AST walk to
        // learn the old name (cached). We rename
        // ALL occurrences of that name across all open documents,
        // mirroring how `references` works. Returns `None` (no rename)
        // for operators, numbers, keywords.
        let Some((current_file, text)) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let Some(byte_offset) = position_to_byte_offset_pos(&text, position) else {
            return Ok(None);
        };
        let Some((old_name, _)) = find_ident_at_offset(&current_file, byte_offset) else {
            return Ok(None);
        };

        // Build the per-URI edit map. For each open document we find
        // every occurrence of `old_name` (including declaration sites,
        // since a rename must update the definition too) and append a
        // `TextEdit` replacing the old name with the new one. Edits
        // are grouped by file URI into the `WorkspaceEdit.changes`
        // map; the editor applies each group atomically per file.
        let mut edits: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for doc_path in &docs {
            let Some((file, doc_text)) = self.parsed_file(doc_path).await else {
                continue;
            };
            let doc_uri = path_to_uri(doc_path);
            // include_declaration = true: a rename must rewrite the
            // definition site as well as every read / call site.
            let locations = find_references_in_file(&file, &old_name, &doc_uri, &doc_text, true);
            for loc in locations {
                edits.entry(doc_uri.clone()).or_default().push(TextEdit {
                    range: loc.range,
                    new_text: new_name.clone(),
                });
            }
        }

        // No occurrences across any open document: report no rename
        // rather than an empty (no-op) workspace edit.
        if edits.is_empty() {
            return Ok(None);
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
        let path = uri_to_path(&uri);
        let position = params.position;

        // Validate that the cursor is on a renameable identifier before
        // the editor shows the rename UI. Use the AST-based finder so we
        // get the exact span of the innermost identifier, then convert
        // it to an LSP range. Returns `None` for operators, numbers,
        // keywords, and whitespace.
        let Some((file, text)) = self.parsed_file(&path).await else {
            return Ok(None);
        };
        let Some(byte_offset) = position_to_byte_offset_pos(&text, position) else {
            return Ok(None);
        };
        let Some((_, span)) = find_ident_at_offset(&file, byte_offset) else {
            return Ok(None);
        };
        let range = span_to_range(&text, span).unwrap();

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

        // Parse the current document (cached). Document
        // highlight is scoped to the current file (per the LSP spec), so
        // we only parse once. `text` comes from the same version the
        // AST was parsed from, so offsets always match.
        let Some((file, text)) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        // Find the identifier under the cursor via an AST walk. Returns
        // `None` (no highlights) for operators, numbers, and keywords.
        let Some(byte_offset) = position_to_byte_offset_pos(&text, position) else {
            return Ok(None);
        };
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

        // Parse the document (cached), pairing the AST with the exact
        // text it was parsed from. On any parse failure we return
        // `None` (no folding ranges) rather than erroring. Mirrors
        // `document_symbol` / `inlay_hint`.
        let Some((file, text)) = self.parsed_file(&path).await else {
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
        let path = uri_to_path(&uri);

        // Parse the document (cached), pairing the AST with the exact
        // text it was parsed from. On any parse failure we return
        // `None` (no selection ranges) rather than erroring. Mirrors
        // `document_symbol` / `folding_range`.
        let Some((file, text)) = self.parsed_file(&path).await else {
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
    /// Apply a single incremental text change (Plan 33 W6).
    ///
    /// For changes with a range (incremental sync), we:
    /// 1. Apply the edit to the old text to produce the new text
    /// 2. Build a tree-sitter `InputEdit` from the range
    /// 3. Edit the old tree-sitter tree and feed it to `parse_with_tree`
    /// 4. Store the new tree for the next incremental parse
    ///
    /// For changes without a range (full text — fallback), we do a full parse.
    async fn apply_incremental_change(
        &self,
        path: &str,
        change: TextDocumentContentChangeEvent,
        version: i32,
    ) {
        if let Some(range) = change.range {
            // Incremental: apply the range edit to the old text.
            let (old_text, old_tree) = {
                let state = self.state.lock().await;
                let old = state.docs.get(path).cloned();
                (old, state.trees.get(path).cloned())
            };

            if let Some(old_text) = old_text {
                // Invalid UTF-16 endpoints (including a position inside an
                // astral surrogate pair) cannot describe a byte splice.
                // Ignore the malformed event rather than clamping it and
                // corrupting the document.
                let Some((start_byte, end_byte)) = range_byte_span(&old_text, range) else {
                    tracing::error!(
                        ?range,
                        "dropping document change with invalid UTF-16 range; server and client text will desynchronize until a full sync is received"
                    );
                    return;
                };
                let new_text = {
                    let mut result = String::with_capacity(old_text.len() + change.text.len());
                    result.push_str(&old_text[..start_byte]);
                    result.push_str(&change.text);
                    result.push_str(&old_text[end_byte..]);
                    result
                };
                let edit =
                    build_input_edit_from_span(&old_text, start_byte, end_byte, &change.text);
                self.update_doc(path.to_string(), new_text, version).await;

                // Build InputEdit and do incremental parse.
                let mut tree_mut = old_tree;
                if let Some(ref mut tree) = tree_mut {
                    tree.edit(&edit);
                }
                // Store the tree for next time so the next parse is incremental.
                let mut state = self.state.lock().await;
                if let Some(tree) = tree_mut {
                    state.trees.insert(path.to_string(), tree);
                } else {
                    state.trees.remove(path);
                }
            } else {
                // No old text — treat as full replacement. Clear stale tree.
                {
                    let mut state = self.state.lock().await;
                    state.trees.remove(path);
                }
                self.update_doc(path.to_string(), change.text, version)
                    .await;
            }
        } else {
            // Full text change (no range). Clear stale tree.
            {
                let mut state = self.state.lock().await;
                state.trees.remove(path);
            }
            self.update_doc(path.to_string(), change.text, version)
                .await;
        }
    }

    async fn update_doc(&self, path: String, text: String, version: i32) {
        let mut state = self.state.lock().await;
        state.docs.insert(path.clone(), text);
        state.versions.insert(path.clone(), version);
        // Invalidate the cached parse and scope; the next read
        // repopulates them.
        state.parsed.remove(&path);
        state.scopes.remove(&path);
    }

    /// Return the parsed `SourceFile` for `path`, reusing the cached
    /// parse when its version matches the latest known version. The
    /// cache is read + repopulated under the state lock; parsing itself
    /// (which needs a non-`Send` `RParser`) happens after releasing the
    /// Return the current AST for `path` together with the exact source
    /// text it was parsed from. Pairing the two is atomic: handlers use
    /// the text for byte-offset / UTF-16 conversions that must match the
    /// AST's span offsets, so a concurrent `didChange` racing the parse
    /// can never yield a stale text applied to a fresher AST (or vice
    /// versa).
    ///
    /// Returns `None` when the path is not an open document or parsing
    /// fails.
    async fn parsed_file(&self, path: &str) -> Option<(Arc<SourceFile>, String)> {
        loop {
            // Fast path: cache hit with matching version. The cache only
            // returns a parse whose recorded version equals the current
            // document version, so `docs[path]` is exactly the text that
            // parse was produced from.
            {
                let state = self.state.lock().await;
                if let Some(file) = state.cached_parse(path) {
                    let text = state.docs.get(path).cloned()?;
                    return Some((file, text));
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
            // W6: Use incremental parse when we have an old tree.
            let old_tree = {
                let state = self.state.lock().await;
                state.trees.get(path).cloned()
            };
            let mut parser = RParser::new().ok()?;
            let file = if let Some(tree) = old_tree {
                let (parsed, new_tree) = parser.parse_with_tree(path, &text, Some(&tree)).ok()?;
                // Store the new tree for the next incremental parse.
                {
                    let mut state = self.state.lock().await;
                    state.trees.insert(path.to_string(), new_tree);
                }
                Arc::new(parsed)
            } else {
                // Full parse: also store the tree so subsequent edits can be incremental.
                let (parsed, new_tree) = parser.parse_with_tree(path, &text, None).ok()?;
                {
                    let mut state = self.state.lock().await;
                    state.trees.insert(path.to_string(), new_tree);
                }
                Arc::new(parsed)
            };
            let mut state = self.state.lock().await;
            // If an edit landed while parsing, retry against the new version
            // instead of returning an AST already known to be stale.
            if state.record_parse(path, version, Arc::clone(&file)) {
                return Some((file, text));
            }
        }
    }

    /// Return the top-level `Scope` for `path`, reusing the cached
    /// single-file `check_with_scope` result when its version matches.
    /// Used by hover/inlay/completion so they don't re-run the check on
    /// every request. Returns `None` when the document
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
        let (file, _) = self.parsed_file(path).await?;
        let parsed_version = {
            let state = self.state.lock().await;
            state
                .parsed
                .get(path)
                .and_then(|(version, cached)| Arc::ptr_eq(cached, &file).then_some(*version))
        };
        let mut checker = ry_checker::Checker::new(path);
        let user_stubs = {
            let state = self.state.lock().await;
            Arc::clone(&state.user_stubs)
        };
        checker.set_user_stubs(user_stubs);
        let (_, scope) = checker.check_with_scope(&file);
        let mut state = self.state.lock().await;
        if let Some(version) = parsed_version {
            let same_parse = state
                .parsed
                .get(path)
                .is_some_and(|(cached_version, cached)| {
                    *cached_version == version && Arc::ptr_eq(cached, &file)
                });
            if state.versions.get(path).copied() == Some(version) && same_parse {
                state
                    .scopes
                    .insert(path.to_string(), (version, scope.clone()));
            }
        }
        Some(scope)
    }

    /// Incrementally update the project and publish diagnostics for every
    /// open document. Publishing all files is required because an edit to a
    /// function definition can change diagnostics in its cross-file callers.
    async fn publish_diagnostics(&self, uri: Url, generation: u64) {
        // Snapshot the open docs under the lock, then drop the lock
        // before running the checker so a slow check doesn't block
        // other LSP requests (e.g. didOpen of a second file).
        // Snapshot document paths + versions (cheap: string keys, i32 values)
        // without cloning every file's full text. Source text is fetched
        // per-document below via `state.docs` lookups only when needed.
        let (path, doc_paths, versions, user_stubs, project, baseline, config_root) = {
            let state = self.state.lock().await;
            (
                uri_to_path(&uri),
                state
                    .docs
                    .keys()
                    .filter(|path| state.eligibility_for_path(path))
                    .cloned()
                    .collect::<Vec<_>>(),
                state.versions.clone(),
                Arc::clone(&state.user_stubs),
                Arc::clone(&state.project),
                state.effective_baseline(),
                state.root.clone(),
            )
        };
        let requested_is_eligible = {
            let state = self.state.lock().await;
            state.eligibility_for_path(&path)
        };
        if !requested_is_eligible {
            self.client
                .publish_diagnostics(uri.clone(), Vec::new(), None)
                .await;
        }

        // Update the persistent multi-file Project from every open document
        // so cross-file calls resolve. Cached parses avoid re-parsing
        // unchanged documents, and ProjectCache forwards only changed
        // snapshots to Project::update_file so pass-1 collection is reused.
        let mut project_files = Vec::with_capacity(doc_paths.len());
        // Cached parses: `parsed_file` reuses the
        // per-document `SourceFile` cached in `State` and only re-parses
        // documents whose version changed since the last request.
        for doc_path in &doc_paths {
            let Some((file, _)) = self.parsed_file(doc_path).await else {
                continue;
            };
            let Some(version) = versions.get(doc_path).copied() else {
                continue;
            };
            project_files.push((doc_path.clone(), version, file));
        }

        // W4: Merge on-disk files from the background indexer. Open
        // documents shadow disk files (the editor's buffer is
        // authoritative), so skip paths already in project_files.
        let disk_files = {
            let state = self.state.lock().await;
            state.disk_files.clone()
        };
        let open_paths: std::collections::HashSet<String> =
            project_files.iter().map(|(p, _, _)| p.clone()).collect();
        for (path, file) in &disk_files {
            if open_paths.contains(path) {
                continue;
            }
            project_files.push((path.clone(), 0, Arc::clone(file)));
        }

        // Hold the project lock through publication. A newer generation may
        // queue while this loop is awaiting the client, but it cannot
        // interleave publications and will therefore publish last.
        let workspace_context = {
            let state = self.state.lock().await;
            state.workspace_context_for_path(&path).cloned()
        };
        let mut project = project.lock().await;
        let checked =
            project.check_with_workspace(project_files, user_stubs, workspace_context.as_ref());
        // An edit that arrived while parsing/checking invalidates this whole
        // project result because every open document is republished below.
        {
            let state = self.state.lock().await;
            if state.diag_generation != generation {
                return;
            }
        }
        let ProjectCheckResult {
            diagnostics: per_file,
            files: checked_files,
        } = checked;
        for (diagnostic_path, mut diagnostics) in per_file {
            // `Project::check` emits every rule, including the ones the
            // default rule set leaves off (RY003). The CLI drops those in
            // its `SeverityFilter`; the LSP has no user severity
            // configuration to apply, so it enforces the same default here.
            // Without this an opt-in rule shows up in the editor but not in
            // `ry check`, which reads as ry contradicting itself.
            // S2/S4: Apply the effective severity filter. For multi-root
            // workspaces, each document uses its owning folder's config.
            let (filter, min_confidence, excludes) =
                {
                    let state = self.state.lock().await;
                    let file_config = state
                        .folder_config_for_path(&diagnostic_path)
                        .unwrap_or(&state.file_config)
                        .clone();
                    let filter = state.merge_filter(&file_config);
                    let min_confidence =
                        state.folder_settings.min_confidence.as_ref().and_then(|s| {
                            match s.as_str() {
                                "low" => Some(ry_checker::Confidence::Low),
                                "medium" => Some(ry_checker::Confidence::Medium),
                                "high" => Some(ry_checker::Confidence::High),
                                _ => None,
                            }
                        });
                    let excludes = ry_config::Excludes::from_config(&file_config);
                    (filter, min_confidence, excludes)
                };
            ry_checker::apply_filter_to_diagnostics(&mut diagnostics, &filter);

            // S2: Apply min-confidence filtering (mirrors --min-confidence).
            if let Some(min) = min_confidence {
                diagnostics.retain(|d| d.confidence >= min);
            }

            // S2: Apply exclude filtering (mirrors `exclude` in ry.toml).
            if !excludes.is_empty() {
                let rel = ry_config::diagnostic_path(&diagnostic_path, config_root.as_deref());
                if excludes.matches(&rel) {
                    continue;
                }
            }

            // S2: Apply baseline subtraction if configured.
            if let Some(ref baseline) = baseline {
                ry_config::subtract_baseline(&mut diagnostics, baseline, config_root.as_deref());
            }

            // Source, comments, spans, and fixes all come from the same
            // `SourceFile` snapshot passed through `ProjectCache` for this
            // check. This includes unopened indexed files and avoids both a
            // disk reread race and an open-buffer/check snapshot mismatch.
            let checked_file = checked_files.get(&diagnostic_path);
            let source_text = checked_file.map(|file| file.source.as_str());
            let (file_level, suppressions) = match checked_file {
                Some(file) => (
                    ry_checker::has_file_suppression_from_comments(&file.comments),
                    ry_checker::parse_suppressions_from_comments(&file.comments, &file.source),
                ),
                None => (false, Vec::new()),
            };
            let diagnostics: Vec<LspDiagnostic> = diagnostics
                .into_iter()
                .filter(|diagnostic| {
                    !file_level && !ry_checker::is_suppressed(diagnostic, &suppressions)
                })
                .map(|diagnostic| match source_text {
                    Some(text) => diagnostic_to_lsp_with_source(&diagnostic, text),
                    None => diagnostic_to_lsp(diagnostic),
                })
                .collect();
            let diagnostic_uri = if diagnostic_path == path {
                uri.clone()
            } else {
                path_to_uri(&diagnostic_path)
            };
            self.client
                .publish_diagnostics(diagnostic_uri, diagnostics, None)
                .await;
        }
    }

    /// W4: Discover and parse all `.R`/`.r` files under the workspace root(s)
    /// in a background task. Results are stored in `state.disk_files` and a
    /// diagnostic refresh is triggered so cross-file calls into unopened
    /// files resolve on the next check.
    async fn spawn_background_index(&self) {
        let (roots_with_config, user_stubs) = {
            let state = self.state.lock().await;
            let roots = if !state.workspace_folders.is_empty() {
                state.workspace_folders.clone()
            } else if let Some(root) = &state.root {
                vec![(root.clone(), state.file_config.clone())]
            } else {
                Vec::new()
            };
            (roots, Arc::clone(&state.user_stubs))
        };
        if roots_with_config.is_empty() {
            return;
        }

        let indexed = tokio::task::spawn_blocking(move || {
            let mut all_disk_files: HashMap<String, Arc<SourceFile>> = HashMap::new();
            let mut contexts = Vec::new();
            for (root, config) in &roots_with_config {
                let files_for_root = crate::index::index_workspace(root, config);
                let files: Vec<&SourceFile> = files_for_root.values().map(AsRef::as_ref).collect();
                match ry_workspace::resolve_workspace_context(
                    root,
                    config,
                    ry_workspace::ResolutionEnvironment {
                        files,
                        user_stubs: &user_stubs,
                    },
                ) {
                    Ok(context) => contexts.push((root.clone(), context)),
                    Err(error) => tracing::warn!(%error, "workspace resolution degraded"),
                }
                all_disk_files.extend(files_for_root);
            }
            contexts.sort_by_key(|(root, _)| std::cmp::Reverse(root.as_os_str().len()));
            (all_disk_files, contexts)
        })
        .await;

        match indexed {
            Ok((disk_files, contexts)) => {
                tracing::info!(
                    files = disk_files.len(),
                    "background workspace index complete"
                );
                let mut state = self.state.lock().await;
                state.disk_files = disk_files;
                state.workspace_contexts = contexts;
            }
            Err(error) => tracing::warn!(%error, "background workspace index failed"),
        }
    }

    /// Debounce diagnostics for `uri`: bump the workspace generation counter
    /// and spawn a task that sleeps ~180ms, then publishes diagnostics
    /// ONLY if its generation is still the latest. A newer edit during
    /// the sleep window bumps the counter and the stale task aborts, so
    /// a burst of keystrokes triggers a single check rather than one per
    /// keystroke.
    async fn schedule_diagnostics(&self, uri: Url) {
        // Diagnostics are project-wide, so one workspace generation
        // coalesces edits in any open document.
        let generation = {
            let mut state = self.state.lock().await;
            state.diag_generation = state.diag_generation.wrapping_add(1);
            state.diag_generation
        };
        let backend = Backend {
            client: self.client.clone(),
            state: Arc::clone(&self.state),
        };
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            // Only publish if no newer edit arrived during the sleep.
            let stale = {
                let state = backend.state.lock().await;
                state.diag_generation != generation
            };
            if !stale {
                backend.publish_diagnostics(uri, generation).await;
            }
        });
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct WorkspaceConfig {
    typeshed: Vec<PathBuf>,
}

fn load_workspace_stubs(
    root: Option<&std::path::Path>,
) -> Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>> {
    let mut merged = std::collections::BTreeMap::new();
    let Some(root) = root else {
        return Arc::new(merged);
    };
    let config_path = root.join("ry.toml");
    let text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Arc::new(merged),
        Err(error) => {
            tracing::warn!(path = %config_path.display(), %error, "failed to read typeshed config");
            return Arc::new(merged);
        }
    };
    let config: WorkspaceConfig = match toml::from_str(&text) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(path = %config_path.display(), %error, "failed to parse typeshed config");
            return Arc::new(merged);
        }
    };
    for dir in config.typeshed {
        let dir = if dir.is_relative() {
            root.join(dir)
        } else {
            dir
        };
        match ry_typeshed::load_stub_dir_with_warnings(&dir) {
            Ok((stubs, warnings)) => {
                merged.extend(stubs);
                for warning in warnings {
                    tracing::warn!(%warning, "skipping malformed user stub");
                }
            }
            Err(error) => tracing::warn!(%error, "failed to load user stub directory"),
        }
    }
    Arc::new(merged)
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

/// Apply an LSP range-based edit to the old source text, producing the
/// new text. LSP positions are 0-based line/character (UTF-16 code units).
/// We convert to byte offsets for the splice.
fn range_byte_span(old_text: &str, range: Range) -> Option<(usize, usize)> {
    let start_byte = position_to_byte_offset_pos(old_text, range.start)?;
    let end_byte = position_to_byte_offset_pos(old_text, range.end)?;
    (start_byte <= end_byte).then_some((start_byte, end_byte))
}



/// Build a tree-sitter `InputEdit` from an LSP range and replacement text.
/// The InputEdit tells tree-sitter which byte range changed and the new
/// positions, so it can reuse unchanged subtrees.
#[cfg(test)]
pub(crate) fn build_input_edit(
    old_text: &str,
    range: Range,
    new_text: &str,
) -> Option<ry_core::InputEdit> {
    let (start_byte, old_end_byte) = range_byte_span(old_text, range)?;
    Some(build_input_edit_from_span(
        old_text,
        start_byte,
        old_end_byte,
        new_text,
    ))
}

fn build_input_edit_from_span(
    old_text: &str,
    start_byte: usize,
    old_end_byte: usize,
    new_text: &str,
) -> ry_core::InputEdit {
    let new_end_byte = start_byte + new_text.len();

    let start_position = byte_offset_to_point(old_text, start_byte);
    let old_end_position = byte_offset_to_point(old_text, old_end_byte);
    let new_end_position = byte_offset_to_point_relative(start_byte, start_position, new_text);

    ry_core::InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position,
        old_end_position,
        new_end_position,
    }
}

/// Convert a byte offset to a tree-sitter Point (row, byte column).
fn byte_offset_to_point(text: &str, byte_offset: usize) -> ry_core::Point {
    let offset = byte_offset.min(text.len());
    let mut row = 0usize;
    let mut last_line_start = 0usize;

    for (i, ch) in text[..offset].char_indices() {
        if ch == '\n' {
            row += 1;
            last_line_start = i + 1;
        }
    }

    let column = offset - last_line_start;
    ry_core::Point { row, column }
}

/// Compute the new end Point after inserting `new_text` at `start_byte`.
fn byte_offset_to_point_relative(
    _start_byte: usize,
    start_position: ry_core::Point,
    new_text: &str,
) -> ry_core::Point {
    let newlines = new_text.matches('\n').count();
    if newlines == 0 {
        ry_core::Point {
            row: start_position.row,
            column: start_position.column + new_text.len(),
        }
    } else {
        let last_newline = new_text.rfind('\n').unwrap();
        ry_core::Point {
            row: start_position.row + newlines,
            column: new_text.len() - last_newline - 1,
        }
    }
}
