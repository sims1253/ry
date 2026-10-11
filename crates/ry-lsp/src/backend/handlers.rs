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

        // Fallback settings: first per-folder entry, else the global ones.
        let fallback_settings = server_settings
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

        let supports_diagnostic_data = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|document| document.publish_diagnostics.as_ref())
            .and_then(|diagnostics| diagnostics.data_support)
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
        // Without workspace folders, the root is the single folder.
        let ws_folder_paths = match (&root, ws_folder_paths.is_empty()) {
            (Some(root), true) => vec![(0, root.clone())],
            _ => ws_folder_paths,
        };
        let settings = server_settings.clone();
        let fallback_root = root.unwrap_or_default();
        let (folder_contexts, fallback) = tokio::task::spawn_blocking(move || {
            (
                load_folder_contexts(&ws_folder_paths, &settings),
                FolderAnalysisContext::load(fallback_root, fallback_settings, None),
            )
        })
        .await
        .unwrap_or_default();

        let mut state = self.state.lock().await;
        state.initial_index_pending = true;
        state.fallback = fallback;
        state.server_settings = server_settings;
        state.supports_workspace_configuration = supports_workspace_configuration;
        state.supports_document_changes = supports_document_changes;
        state.supports_diagnostic_data = supports_diagnostic_data;
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

        // didOpen can run while indexing. Replay those documents only after
        // their workspace bindings and package metadata have been installed.
        if self.reindex().await {
            self.republish_all_open_documents().await;
        }
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
            let previous = state
                .docs
                .get(&path)
                .cloned()
                .zip(state.versions.get(&path).copied());
            let open = state.open_source_paths.entry(path.clone()).or_default();
            let collides = !open.is_empty() && (!open.contains_key(&uri) || open.len() > 1);
            if collides && open.len() == 1 {
                open.values_mut().next().expect("one open source").shadow = previous;
            }
            open.insert(
                uri.clone(),
                super::OpenSource {
                    native: uri.to_file_path().ok(),
                    shadow: collides.then(|| (text.clone(), version)),
                },
            );
            if collides {
                state.active_collision_uri.insert(path.clone(), uri.clone());
            }
            state.update_doc(&path, text, version);
        }
        self.request_pass([path]).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let version = params.text_document.version;
        #[cfg(feature = "test-util")]
        let paused_transition = crate::test_seam::pause(Seam::ChangeTransition).await;
        let applied = self.state.lock().await.apply_document_changes(
            &path,
            &uri,
            params.content_changes,
            version,
        );
        #[cfg(feature = "test-util")]
        if paused_transition {
            crate::test_seam::landed(Seam::ChangeTransition);
        }
        if !applied {
            return;
        }
        self.request_pass([path]).await;
        // Test seam: signals didChange completion (see `test_seam`).
        #[cfg(feature = "test-util")]
        crate::test_seam::signal(Seam::DidChange);
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
                let state = &mut *state;
                for ctx in state
                    .folder_contexts
                    .iter_mut()
                    .chain(std::iter::once(&mut state.fallback))
                {
                    ctx.folder_settings = settings.clone();
                    ctx.refresh_policy();
                }
                state.server_settings.global_settings = settings;
            }
        }

        self.reload_folder_contexts().await;
        self.refresh_watchers().await;
        self.reindex().await;

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

        // Clear every URI under a removed root: open documents and closed
        // disk files alike, since the tracked map forgets them below.
        let (docs_to_clear, docs_to_republish): (Vec<Url>, Vec<String>) = {
            let state = self.state.lock().await;
            let (keep, clear): (Vec<&String>, Vec<&String>) =
                state.docs.keys().partition(|p| !under_removed_root(p));
            let source_uris = |path: &str| source_uris(&state.open_source_paths, path);
            let mut docs_to_clear: std::collections::HashSet<Url> =
                clear.into_iter().flat_map(|p| source_uris(p)).collect();
            for (_, uris) in state
                .published_uris
                .iter()
                .filter(|(path, _)| under_removed_root(path.as_str()))
            {
                docs_to_clear.extend(uris.iter().cloned());
            }
            (
                docs_to_clear.into_iter().collect(),
                keep.into_iter().cloned().collect(),
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
            state.reconciliation.cancel_where(under_removed_root);
            state.published_uris.retain(|p, _| !under_removed_root(p));

            let added_roots: Vec<PathBuf> = params
                .event
                .added
                .iter()
                .filter_map(|f| f.uri.to_file_path().ok())
                .collect();

            let new_contexts = if !added_roots.is_empty() {
                let server_settings = state.server_settings.clone();
                tokio::task::spawn_blocking(move || {
                    let added: Vec<_> = added_roots
                        .into_iter()
                        .map(|path| (usize::MAX, path))
                        .collect();
                    load_folder_contexts(&added, &server_settings)
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
            if state.folder_contexts.is_empty() {
                state.initial_index_pending = false;
            }
        }

        self.refresh_watchers().await;
        self.reindex().await;

        for uri in &docs_to_clear {
            self.client
                .publish_diagnostics(uri.clone(), Vec::new(), None)
                .await;
        }

        self.request_pass(docs_to_republish).await;
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
                    || change.uri.to_file_path().is_ok_and(|path| {
                        path.parent().and_then(std::path::Path::file_name)
                            == Some(std::ffi::OsStr::new("src"))
                            && matches!(
                                path.extension().and_then(|ext| ext.to_str()),
                                Some("c" | "cc" | "cpp" | "cxx")
                            )
                    })
                    || path.ends_with(".rda")
                    || path.ends_with(".RData")
                    || path.ends_with(".rdata")
            });
        // Source events refresh the disk index entry by entry; the refresh
        // itself skips open buffers.
        let r_source_paths: Vec<std::path::PathBuf> = params
            .changes
            .iter()
            .filter_map(|change| change.uri.to_file_path().ok())
            .filter(|path| ry_workspace::is_source_path(path))
            .collect();
        if !resolution_changed && r_source_paths.is_empty() {
            return;
        }

        if config_or_baseline_changed {
            self.reload_folder_contexts().await;
        }

        // Closed files to republish alongside every open document.
        let mut publish: Vec<String> = Vec::new();
        if resolution_changed {
            // A superseded scan leaves the republish to its supplanter.
            if self.reindex().await {
                publish.extend(self.state.lock().await.published_uris.keys().cloned());
            }
        } else {
            let mut landed: Vec<(String, u64)> = Vec::new();
            for path in r_source_paths {
                let path_string = path.to_string_lossy().into_owned();
                if let Some(epoch) = self.refresh_disk_entry(path).await {
                    landed.push((path_string, epoch));
                }
            }
            if landed.is_empty() {
                // Retained obligations converge through the driver.
                self.wake_reconciliation().await;
                return;
            }
            // Advance the owning package contexts before publishing.
            let ctx_settled = self.refresh_package_contexts(&landed).await;
            publish.extend(landed.into_iter().map(|(path, _)| path));
            self.acknowledge_publications(&ctx_settled).await;
        }

        publish.extend(self.state.lock().await.docs.keys().cloned());
        self.request_pass(publish).await;
        self.wake_reconciliation().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let (remaining_open_paths, survivor) = {
            let mut state = self.state.lock().await;
            let survivor = state.close_open_source(&path, &uri);
            // The empty publication below clears this URI.
            if survivor.is_none() {
                state.docs.remove(&path);
                state.versions.remove(&path);
                state.parsed.remove(&path);
                state.hints.remove(&path);
                state.trees.remove(&path);
                state.published_uris.remove(&path);
            } else if let Some(uris) = state.published_uris.get_mut(&path) {
                uris.remove(&uri);
                if uris.is_empty() {
                    state.published_uris.remove(&path);
                }
            }
            // Invalidate any in-flight debounced publish for this file.
            state.diag_generation = state.diag_generation.wrapping_add(1);
            (state.docs.keys().cloned().collect::<Vec<_>>(), survivor)
        };
        if let Some(survivor) = survivor {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
            self.request_pass([uri_to_path(&survivor)]).await;
            return;
        }
        // Re-read the closed file so its last-saved bytes become the indexed
        // snapshot, converging with a fresh server.
        let refreshed_epoch = self
            .refresh_disk_entry(std::path::PathBuf::from(&path))
            .await;
        if let Some(epoch) = refreshed_epoch {
            // The close's own publications below carry the publication duty.
            let ctx_settled = self.refresh_package_contexts(&[(path, epoch)]).await;
            self.acknowledge_publications(&ctx_settled).await;
        }
        self.client
            .publish_diagnostics(uri.clone(), Vec::new(), None)
            .await;
        // Closing a document can change diagnostics in the remaining open
        // documents (names defined in the closed file become unresolved),
        // so refresh them.
        if !remaining_open_paths.is_empty() {
            self.request_pass(remaining_open_paths).await;
        }
        self.wake_reconciliation().await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        // Cancel pending work before any driver can claim another refresh.
        let mut state = self.state.lock().await;
        state.reconciliation.shutdown();
        Ok(())
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let range = params.range;

        // Hints are document-local, so they ignore the size and depth caps
        // but not a disabled folder or an unopted report.
        if self.state.lock().await.analysis_disabled(&path) {
            return Ok(None);
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
        if ry_workspace::reports::is_report_path(std::path::Path::new(&path)) {
            // Masked prose and cell metadata are not editable R source.
            return Ok(None);
        }

        {
            let state = self.state.lock().await;
            if !state.eligibility_for_path(&path) || state.has_collision(&path) {
                // A display-keyed parse can belong to another native URI.
                // Even matching document version numbers do not prove that
                // an edit uses the requesting buffer's source text.
                return Ok(None);
            }
        }
        let Some((file, _)) = self.parsed_file(&path).await else {
            return Ok(None);
        };

        let (versioned_edits, required_origin) = {
            let state = self.state.lock().await;
            let Some((version, cached)) = state.parsed.get(&path) else {
                return Ok(None);
            };
            if !Arc::ptr_eq(cached, &file)
                || state.versions.get(&path) != Some(version)
                || !state.eligibility_for_path(&path)
                || state.has_collision(&path)
            {
                return Ok(None);
            }
            (
                state.supports_document_changes.then_some(*version),
                state
                    .supports_diagnostic_data
                    .then(|| diagnostic_origin(&path, *version, state.diag_generation)),
            )
        };

        let diagnostics: Vec<_> = params
            .context
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.source.as_deref() == Some("ry"))
            .filter(|diagnostic| {
                required_origin
                    .as_ref()
                    .is_none_or(|origin| diagnostic.data.as_ref() == Some(origin))
            })
            .collect();
        // Stale ranges must not suppress unrelated current lines. Requiring
        // preserved data is safe only for clients that negotiated it.
        if diagnostics.is_empty() {
            return Ok(None);
        }

        // One quick-fix per diagnostic visible at the cursor; helpers skip
        // lines that already carry a suppression.
        let mut actions: CodeActionResponse = Vec::new();
        for diag in diagnostics {
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
