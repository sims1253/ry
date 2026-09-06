//! LSP request and notification handlers.

use super::*;

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let root = params.root_uri.and_then(|uri| uri.to_file_path().ok());

        // initializationOptions is the only settings channel Zed can
        // drive, so it must be sufficient on its own. The shape mirrors
        // ruff-vscode's: per-folder settings plus a global fallback.
        let server_settings: ServerSettings = params
            .initialization_options
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Root-level folder_settings: first per-folder entry, else the
        // global fallback.
        let folder_settings = server_settings
            .settings
            .first()
            .cloned()
            .unwrap_or_else(|| server_settings.global_settings.clone());

        let supports_workspace_configuration = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.configuration)
            .unwrap_or(false);

        let supports_document_changes = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.workspace_edit.as_ref())
            .and_then(|edit| edit.document_changes)
            .unwrap_or(false);

        let supports_did_change_watched_files = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.did_change_watched_files.as_ref())
            .and_then(|f| f.dynamic_registration)
            .unwrap_or(false);

        let supports_relative_patterns = params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|w| w.did_change_watched_files.as_ref())
            .and_then(|f| f.relative_pattern_support)
            .unwrap_or(false);
        let server_settings_clone = server_settings.clone();
        let root_clone = root.clone();
        let ws_folder_paths: Vec<(usize, PathBuf)> = params
            .workspace_folders
            .as_ref()
            .map(|folders| {
                folders
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, f)| f.uri.to_file_path().ok().map(|path| (idx, path)))
                    .collect()
            })
            .unwrap_or_default();
        let folder_contexts = {
            tokio::task::spawn_blocking(move || {
                build_folder_contexts(
                    root_clone.as_deref(),
                    &ws_folder_paths,
                    &server_settings_clone,
                )
            })
            .await
            .unwrap_or_default()
        };

        // Root-level config and stubs for the single-root fallback.
        let root_clone2 = root.clone();
        let (file_config, user_stubs) =
            tokio::task::spawn_blocking(move || load_root_config_and_stubs(root_clone2.as_deref()))
                .await
                .unwrap_or_default();

        let root_baseline =
            match load_folder_baseline(&folder_settings, &file_config, root.as_deref()) {
                Ok(opt) => opt,
                Err(error) => {
                    tracing::warn!(%error, "failed to load root baseline; no baseline cached");
                    None
                }
            };

        let (root_filter, root_min_confidence, root_excludes) =
            compute_folder_filter(&file_config, &folder_settings);

        let mut state = self.state.lock().await;
        state.user_stubs = user_stubs;
        state.root = root;
        state.file_config = file_config;
        state.root_baseline = root_baseline;
        state.root_filter = root_filter;
        state.root_min_confidence = root_min_confidence;
        state.root_excludes = root_excludes;
        state.folder_settings = folder_settings;
        state.server_settings = server_settings;
        state.supports_workspace_configuration = supports_workspace_configuration;
        state.supports_document_changes = supports_document_changes;
        state.supports_did_change_watched_files = supports_did_change_watched_files;
        state.supports_relative_patterns = supports_relative_patterns;
        state.folder_contexts = folder_contexts;
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // All position conversion in this server is UTF-16. Advertise
                // it explicitly instead of relying on the protocol default.
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    // Incremental sync: the client sends
                    // only the edited range, which we use to build a
                    // tree-sitter InputEdit for incremental reparse.
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                // Inlay hints are the primary way users see the checker's
                // inference: R has no annotation syntax to attach types to.
                inlay_hint_provider: Some(OneOf::Left(true)),
                // Quick fixes: per-line `# ry: ignore[CODE]` suppressions
                // plus a file-level `# ry: ignore-file` action.
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                // Multi-root workspace folder support.
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

        // If the client supports workspace/configuration, pull the
        // `ry.*` section now. This is the primary settings path for VS
        // Code and supersedes whatever was in initializationOptions.
        let should_pull = {
            let state = self.state.lock().await;
            state.supports_workspace_configuration
        };
        if should_pull {
            self.pull_folder_settings().await;
            self.reload_folder_contexts().await;
        }

        self.refresh_watchers().await;

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
        // If any change has an invalid UTF-16 range, abort the remaining
        // batch: subsequent changes' ranges are relative to the client
        // text after the dropped edit, so applying them to the server's
        // (pre-dropped-edit) text would splice wrong bytes.
        for change in params.content_changes {
            if !self.apply_incremental_change(&path, change, version).await {
                tracing::error!(
                    "aborting remaining changes in didChange batch for {path}; server and client text will desynchronize until a full sync is received"
                );
                break;
            }
        }
        self.schedule_diagnostics(uri).await;
        // Test seam: signals didChange completion (see `test_seam`).
        #[cfg(feature = "test-util")]
        crate::test_seam::note_did_change();
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // Pull-capable clients re-pull `ry.*`; others send settings inline.
        let should_pull = {
            let state = self.state.lock().await;
            state.supports_workspace_configuration
        };

        if should_pull {
            self.pull_folder_settings().await;
        } else {
            // The client sent settings inline. They may be wrapped in
            // an outer "ry" key (VS Code) or be the raw ry settings.
            let raw = &params.settings;
            let ry_section = raw.get("ry").unwrap_or(raw);
            if let Ok(settings) = serde_json::from_value::<FolderSettings>(ry_section.clone()) {
                let mut state = self.state.lock().await;
                state.folder_settings = settings.clone();
                for ctx in &mut state.folder_contexts {
                    ctx.folder_settings = settings.clone();
                }
                state.server_settings.global_settings = settings;
                refresh_cached_folder_filters(&mut state);
            }
        }

        self.reload_folder_contexts().await;
        self.refresh_watchers().await;
        self.spawn_background_index().await;

        self.republish_all_open_documents().await;
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        // Remove all state owned by removed roots, rebuild the sorted
        // folder contexts, reindex, then republish only after the new
        // state is installed.
        let removed_paths: Vec<PathBuf> = params
            .event
            .removed
            .iter()
            .filter_map(|f| f.uri.to_file_path().ok())
            .collect();
        let under_removed_root = |p: &str| {
            removed_paths
                .iter()
                .any(|r| std::path::Path::new(p).starts_with(r))
        };

        // URIs for open documents owned by removed roots — diagnostics for
        // these must be cleared.
        let (docs_to_clear, docs_to_republish): (Vec<Url>, Vec<Url>) = {
            let state = self.state.lock().await;
            let (keep, clear): (Vec<&String>, Vec<&String>) =
                state.docs.keys().partition(|p| !under_removed_root(p));
            (
                clear.into_iter().map(|p| path_to_uri(p)).collect(),
                keep.into_iter().map(|p| path_to_uri(p)).collect(),
            )
        };

        {
            let mut state = self.state.lock().await;

            // Remove state owned by removed roots BEFORE rebuilding so
            // stale state never enters the next check.
            state.disk_files.retain(|p, _| !under_removed_root(p));
            state.trees.retain(|p, _| !under_removed_root(p));
            state.parsed.retain(|p, _| !under_removed_root(p));
            state.hints.retain(|p, _| !under_removed_root(p));

            // Rebuild folder contexts from the surviving + added roots
            // through the shared builder used at initialize.
            let added_roots: Vec<PathBuf> = params
                .event
                .added
                .iter()
                .filter_map(|f| f.uri.to_file_path().ok())
                .collect();

            let new_contexts = if !added_roots.is_empty() {
                let server_settings = state.server_settings.clone();
                tokio::task::spawn_blocking(move || {
                    build_folder_contexts(
                        None,
                        &added_roots
                            .iter()
                            .map(|path| (usize::MAX, path.clone()))
                            .collect::<Vec<_>>(),
                        &server_settings,
                    )
                })
                .await
                .unwrap_or_default()
            } else {
                Vec::new()
            };

            // Replacing removed contexts drops their project caches with
            // them; surviving contexts keep theirs, new ones start fresh.
            state
                .folder_contexts
                .retain(|ctx| !removed_paths.iter().any(|p| p == &ctx.root));
            state.folder_contexts.extend(new_contexts);
            // Sort by root path length descending for longest-prefix matching.
            state
                .folder_contexts
                .sort_by_key(|ctx| std::cmp::Reverse(ctx.root.as_os_str().len()));

            state.index_generation = state.index_generation.wrapping_add(1);
        }

        self.refresh_watchers().await;

        // Skip indexing when no folder contexts remain (all removed) so
        // state.root does not re-index a removed directory.
        let has_contexts = !self.state.lock().await.folder_contexts.is_empty();
        if has_contexts {
            self.spawn_background_index().await;
        }

        for uri in &docs_to_clear {
            self.client
                .publish_diagnostics(uri.clone(), Vec::new(), None)
                .await;
        }

        // Not `republish_all_open_documents`: documents under removed roots
        // were just cleared and, falling back to root-level eligibility, a
        // blanket reschedule would re-publish results for them.
        for uri in &docs_to_republish {
            self.schedule_diagnostics(uri.clone()).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        // Refresh configuration and filesystem-backed resolution when any
        // registered package metadata, data, stub, or config file changes.
        let config_paths = custom_config_paths(&*self.state.lock().await);
        let config_or_baseline_changed = params.changes.iter().any(|change| {
            let path = change.uri.path();
            path.ends_with("ry.toml")
                || path.ends_with(".json")
                || change
                    .uri
                    .to_file_path()
                    .is_ok_and(|path| config_paths.contains(&path))
        });
        let resolution_changed = config_or_baseline_changed
            || params.changes.iter().any(|change| {
                let path = change.uri.path();
                path.ends_with("DESCRIPTION")
                    || path.ends_with("NAMESPACE")
                    || path.ends_with(".rda")
                    || path.ends_with(".RData")
                    || path.ends_with(".rdata")
            });
        if !resolution_changed {
            return;
        }

        if config_or_baseline_changed {
            self.reload_folder_contexts().await;
        }

        self.spawn_background_index().await;

        self.republish_all_open_documents().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let remaining_open_paths = {
            let mut state = self.state.lock().await;
            state.docs.remove(&path);
            state.versions.remove(&path);
            state.parsed.remove(&path);
            state.hints.remove(&path);
            state.trees.remove(&path);
            // Invalidate any in-flight debounced publish for this file.
            state.diag_generation = state.diag_generation.wrapping_add(1);
            state.docs.keys().cloned().collect::<Vec<_>>()
        };
        {
            let (root_project, folder_project_opt) = {
                let state = self.state.lock().await;
                let root = Arc::clone(&state.project);
                let folder = state
                    .folder_context_for_path(&path)
                    .map(|ctx| Arc::clone(&ctx.project_cache));
                (root, folder)
            };
            let mut project = root_project.lock().await;
            project.project.remove_file(&path);
            project.files.remove(&path);
            if let Some(folder_proj) = folder_project_opt {
                let mut folder_proj = folder_proj.lock().await;
                folder_proj.project.remove_file(&path);
                folder_proj.files.remove(&path);
            }
        }
        // Clear diagnostics for the closed document so stale squiggles
        // don't linger after the user closes the file.
        self.client
            .publish_diagnostics(uri.clone(), Vec::new(), None)
            .await;
        // Closing a document can change diagnostics in the remaining open
        // documents (names defined in the closed file become unresolved),
        // so refresh them.
        if let Some(first) = remaining_open_paths.first() {
            self.schedule_diagnostics(path_to_uri(first)).await;
        }
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let range = params.range;

        // Inlay hints are on-demand analysis, so the same eligibility
        // gate as the publish path applies: a folder set to `enable:
        // false` (or a discovery-excluded file) gets no hints.
        {
            let state = self.state.lock().await;
            if !state.eligibility_for_path(&path) {
                return Ok(None);
            }
        }

        let Some(mut hints) = self.hints_for(&path).await else {
            return Ok(None);
        };

        // Filter to the visible range; off-screen hints are dropped.
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

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        if params.context.only.as_ref().is_some_and(|kinds| {
            !kinds
                .iter()
                .any(|kind| *kind == CodeActionKind::EMPTY || *kind == CodeActionKind::QUICKFIX)
        }) {
            return Ok(None);
        }
        if !params
            .context
            .diagnostics
            .iter()
            .any(|diag| diag.source.as_deref() == Some("ry"))
        {
            return Ok(None);
        }
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);

        if !self.state.lock().await.eligibility_for_path(&path) {
            return Ok(None);
        }
        let Some((file, _)) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        let versioned_edits = {
            let state = self.state.lock().await;
            let Some((version, cached)) = state.parsed.get(&path) else {
                return Ok(None);
            };
            if !Arc::ptr_eq(cached, &file)
                || state.versions.get(&path) != Some(version)
                || !state.eligibility_for_path(&path)
            {
                return Ok(None);
            }
            state.supports_document_changes.then_some(*version)
        };

        // One quick-fix per diagnostic visible at the cursor; helpers skip
        // lines that already carry a suppression.
        let mut actions: CodeActionResponse = Vec::new();
        for diag in params
            .context
            .diagnostics
            .iter()
            .filter(|diag| diag.source.as_deref() == Some("ry"))
        {
            if let Some(action) = make_ignore_action(&uri, diag, &file) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        if let Some(action) = make_ignore_file_action(&uri, &file) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }

        if let Some(version) = versioned_edits {
            for action in &mut actions {
                if let CodeActionOrCommand::CodeAction(action) = action
                    && let Some(edit) = &mut action.edit
                    && let Some(changes) = edit.changes.take()
                {
                    edit.document_changes = Some(DocumentChanges::Edits(
                        changes
                            .into_iter()
                            .map(|(uri, edits)| TextDocumentEdit {
                                text_document: OptionalVersionedTextDocumentIdentifier {
                                    uri,
                                    version: Some(version),
                                },
                                edits: edits.into_iter().map(OneOf::Left).collect(),
                            })
                            .collect(),
                    ));
                }
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}
