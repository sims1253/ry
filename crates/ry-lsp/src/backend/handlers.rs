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
        let supports_related_information = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|document| document.publish_diagnostics.as_ref())
            .and_then(|diagnostics| diagnostics.related_information)
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
        state.fallback = fallback;
        state.server_settings = server_settings;
        state.supports_workspace_configuration = supports_workspace_configuration;
        state.supports_document_changes = supports_document_changes;
        state.supports_diagnostic_data = supports_diagnostic_data;
        state.supports_related_information = supports_related_information;
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

        // If the client supports workspace/configuration, the writer pulls
        // the `ry.*` section before the first scan. This is the primary
        // settings path for VS Code and supersedes initializationOptions.
        let should_pull = self.state.lock().await.supports_workspace_configuration;
        // Documents opened meanwhile publish only after the first scan
        // installed their workspace bindings and package metadata.
        self.send(Work::Configure {
            settings: should_pull.then_some(SettingsUpdate::Pull),
            reload: should_pull,
        });
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let text = params.text_document.text.clone();
        let version = params.text_document.version;
        let mut state = self.state.lock().await;
        // Clear any stale tree from a previous session for this path.
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
        state.diag_generation = state.diag_generation.wrapping_add(1);
        self.send(Work::Publish(path));
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let version = params.text_document.version;
        #[cfg(feature = "test-util")]
        let paused_transition = crate::test_seam::pause(Seam::ChangeTransition).await;
        {
            let mut state = self.state.lock().await;
            if state.apply_document_changes(&path, &uri, params.content_changes, version) {
                state.diag_generation = state.diag_generation.wrapping_add(1);
                self.send(Work::Publish(path));
                // Test seam: signals didChange completion (see `test_seam`).
                #[cfg(feature = "test-util")]
                crate::test_seam::signal(Seam::DidChange);
            }
        }
        #[cfg(feature = "test-util")]
        if paused_transition {
            crate::test_seam::landed(Seam::ChangeTransition);
        }
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // Pull-capable clients re-pull `ry.*`; others send settings inline.
        let should_pull = self.state.lock().await.supports_workspace_configuration;
        let settings = if should_pull {
            Some(SettingsUpdate::Pull)
        } else {
            // The client sent settings inline. They may be wrapped in
            // an outer "ry" key (VS Code) or be the raw ry settings.
            let raw = &params.settings;
            let ry_section = raw.get("ry").unwrap_or(raw);
            serde_json::from_value::<FolderSettings>(ry_section.clone())
                .ok()
                .map(SettingsUpdate::Inline)
        };
        self.send(Work::Configure {
            settings,
            reload: true,
        });
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let paths = |folders: &[WorkspaceFolder]| -> Vec<PathBuf> {
            folders
                .iter()
                .filter_map(|folder| folder.uri.to_file_path().ok())
                .collect()
        };
        self.send(Work::Folders {
            added: paths(&params.event.added),
            removed: paths(&params.event.removed),
        });
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.send(Work::Watched(params.changes));
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let path = uri_to_path(&uri);
        let mut state = self.state.lock().await;
        let shared = state.close_open_source(&path, &uri).is_some();
        if !shared {
            state.docs.remove(&path);
            state.versions.remove(&path);
            state.parsed.remove(&path);
            state.hints.remove(&path);
            state.trees.remove(&path);
        }
        state.diag_generation = state.diag_generation.wrapping_add(1);
        self.send(Work::Closed { uri, path, shared });
    }

    async fn shutdown(&self) -> LspResult<()> {
        // Answered once the writer finished its current work, so no
        // publication follows the response.
        let (done, stopped) = tokio::sync::oneshot::channel();
        self.send(Work::Shutdown(done));
        let _ = stopped.await;
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
