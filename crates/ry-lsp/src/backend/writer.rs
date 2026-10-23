//! The index writer: the one task that writes the disk index, the folder
//! analysis contexts, and the publication record, and the only place
//! diagnostics are published.
//!
//! Handlers update open documents themselves, because requests must see
//! the latest buffer, and then queue [`Work`]. Everything that follows
//! from an event runs here, one item at a time, in arrival order, so no
//! write needs a generation guard.

use super::*;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

/// Quiet period after the last publication request before a pass runs.
const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(180);

pub(super) enum Work {
    /// `initialized` or a configuration change: apply the settings, reload
    /// the folder contexts when asked, rescan, and republish open documents.
    Configure {
        settings: Option<SettingsUpdate>,
        reload: bool,
    },
    Folders {
        added: Vec<PathBuf>,
        removed: Vec<PathBuf>,
    },
    Watched(Vec<FileEvent>),
    /// An open document changed.
    Publish(String),
    /// `uri` closed at display path `path`. `shared` when another URI is
    /// still open there.
    Closed {
        uri: Url,
        path: String,
        shared: bool,
    },
    /// Stop after the work queued before it, dropping any pending pass.
    /// The reply means nothing more will be published.
    Shutdown(oneshot::Sender<()>),
}

pub(super) enum SettingsUpdate {
    /// Pull `workspace/configuration` for the folders installed when the
    /// work runs, so a folder change queued before it is covered.
    Pull,
    /// Settings sent inline by the client apply to every context.
    Inline(FolderSettings),
}

/// What a pass sent for one display path: every destination URI, and those
/// that received diagnostics.
type Published = (String, HashSet<Url>, HashSet<Url>);

pub(super) struct IndexWriter {
    client: Client,
    state: Arc<Mutex<State>>,
    work: mpsc::UnboundedReceiver<Work>,
    /// Paths requested since the last pass, and when the next pass runs.
    pending: HashSet<String>,
    deadline: Option<Instant>,
    /// Passes wait for the first scan so they see the workspace bindings.
    indexed: bool,
    /// URIs whose last publication carried diagnostics, keyed by display
    /// path. A URI that stops receiving publications gets an empty one.
    published_uris: HashMap<String, HashSet<Url>>,
}

impl IndexWriter {
    pub(super) fn new(
        client: Client,
        state: Arc<Mutex<State>>,
        work: mpsc::UnboundedReceiver<Work>,
    ) -> Self {
        IndexWriter {
            client,
            state,
            work,
            pending: HashSet::new(),
            deadline: None,
            indexed: false,
            published_uris: HashMap::new(),
        }
    }

    /// Process work until shutdown or until every handler is gone. A due
    /// pass runs only when no work is queued, so it never publishes a state
    /// a queued event is about to change.
    pub(super) async fn run(mut self) {
        loop {
            let work = match self.deadline.filter(|_| self.indexed) {
                Some(deadline) => match tokio::time::timeout_at(deadline, self.work.recv()).await {
                    Ok(work) => work,
                    Err(_) => {
                        self.publish_pass().await;
                        continue;
                    }
                },
                None => self.work.recv().await,
            };
            let Some(work) = work else { return };
            if !self.handle(work).await {
                return;
            }
        }
    }

    /// Apply one item of work; false once the writer stopped.
    async fn handle(&mut self, work: Work) -> bool {
        match work {
            Work::Configure { settings, reload } => {
                if let Some(settings) = settings {
                    self.apply_settings(settings).await;
                }
                if reload {
                    self.reload_folder_contexts().await;
                }
                // Registered before the scan reads, so an edit after the
                // read produces an event.
                refresh_watchers(self.client.clone(), Arc::clone(&self.state)).await;
                self.rescan().await;
                let open = self.state.lock().await.docs.paths();
                self.request(open).await;
            }
            Work::Folders { added, removed } => self.change_folders(added, removed).await,
            Work::Watched(changes) => self.watched_files(changes).await,
            Work::Publish(path) => self.request([path]).await,
            Work::Closed { uri, path, shared } => self.close(uri, path, shared).await,
            Work::Shutdown(done) => {
                let _ = done.send(());
                return false;
            }
        }
        true
    }

    async fn close(&mut self, uri: Url, path: String, shared: bool) {
        if shared {
            if let Some(uris) = self.published_uris.get_mut(&path) {
                uris.remove(&uri);
                if uris.is_empty() {
                    self.published_uris.remove(&path);
                }
            }
            self.clear([uri]).await;
            self.request([path]).await;
            return;
        }
        self.published_uris.remove(&path);
        // The close's empty publication is this path's last word unless
        // another open document requests a pass below.
        self.pending.remove(&path);
        if self.pending.is_empty() {
            self.deadline = None;
        }
        // The last-saved bytes become the indexed snapshot, converging
        // with a fresh server.
        if self.refresh_disk_entry(Path::new(&path)).await {
            self.refresh_package_contexts(std::slice::from_ref(&path))
                .await;
        }
        self.clear([uri]).await;
        // Names the closed buffer defined can change diagnostics in the
        // documents still open.
        let open = self.state.lock().await.docs.paths();
        if !open.is_empty() {
            self.request(open).await;
        }
    }

    async fn apply_settings(&mut self, settings: SettingsUpdate) {
        match settings {
            SettingsUpdate::Pull => {
                let Some(values) = self.pull_folder_settings().await else {
                    return;
                };
                let mut state = self.state.lock().await;
                let state = &mut *state;
                for (root, value) in values {
                    let parsed = serde_json::from_value::<FolderSettings>(value).ok();
                    let ctx = match &root {
                        Some(root) => state
                            .folder_contexts
                            .iter_mut()
                            .find(|ctx| &ctx.root == root),
                        None => Some(&mut state.fallback),
                    };
                    let Some(ctx) = ctx else { continue };
                    if let Some(settings) = parsed {
                        if root.is_none() {
                            state.server_settings.global_settings = settings.clone();
                        }
                        ctx.folder_settings = settings;
                    }
                    ctx.refresh_policy();
                }
            }
            SettingsUpdate::Inline(settings) => {
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
    }

    /// Pull `ry` settings for each folder root, plus a last item for the
    /// fallback context with root `None`. `None` when the request failed.
    async fn pull_folder_settings(&self) -> Option<Vec<(Option<PathBuf>, serde_json::Value)>> {
        let scopes: Vec<(Option<PathBuf>, PathBuf)> = {
            let state = self.state.lock().await;
            state
                .folder_contexts
                .iter()
                .map(|ctx| (Some(ctx.root.clone()), ctx.root.clone()))
                .chain(std::iter::once((None, state.fallback.root.clone())))
                .collect()
        };
        let items = scopes
            .iter()
            .map(|(_, root)| ConfigurationItem {
                scope_uri: Url::from_file_path(root).ok(),
                section: Some("ry".to_string()),
            })
            .collect();
        match self.client.configuration(items).await {
            Ok(values) => Some(
                scopes
                    .into_iter()
                    .map(|(root, _)| root)
                    .zip(values)
                    .collect(),
            ),
            Err(e) => {
                tracing::warn!("workspace/configuration pull failed: {e}");
                None
            }
        }
    }

    /// Reload every context off the runtime; each keeps its last valid
    /// config, baseline, and stubs on failure.
    async fn reload_folder_contexts(&self) {
        let (contexts, fallback) = {
            let state = self.state.lock().await;
            (state.folder_contexts.clone(), state.fallback.clone())
        };
        let reloaded = tokio::task::spawn_blocking(move || {
            let reload = |ctx: &FolderAnalysisContext| {
                FolderAnalysisContext::load(
                    ctx.root.clone(),
                    ctx.folder_settings.clone(),
                    Some(ctx),
                )
            };
            (
                contexts.iter().map(reload).collect::<Vec<_>>(),
                reload(&fallback),
            )
        })
        .await;
        let (contexts, fallback) = match reloaded {
            Ok(reloaded) => reloaded,
            Err(error) => {
                tracing::warn!(%error, "folder context reload task failed; retaining previous contexts");
                return;
            }
        };
        let mut state = self.state.lock().await;
        state.folder_contexts = contexts;
        state.fallback = fallback;
        tracing::info!("workspace config/baseline reloaded");
    }

    /// Drop everything owned by removed roots, add the new roots, rescan,
    /// and republish what remains.
    async fn change_folders(&mut self, added: Vec<PathBuf>, removed: Vec<PathBuf>) {
        let under_removed_root =
            |path: &str| removed.iter().any(|root| Path::new(path).starts_with(root));
        // Clear every URI under a removed root, open or not.
        let (to_clear, to_republish, server_settings) = {
            let state = self.state.lock().await;
            let (clear, keep): (Vec<String>, Vec<String>) = state
                .docs
                .paths()
                .into_iter()
                .partition(|path| under_removed_root(path));
            let mut to_clear: HashSet<Url> = clear
                .iter()
                .flat_map(|path| state.docs.uris(path))
                .collect();
            for (_, uris) in self
                .published_uris
                .iter()
                .filter(|(path, _)| under_removed_root(path))
            {
                to_clear.extend(uris.iter().cloned());
            }
            (to_clear, keep, state.server_settings.clone())
        };
        let added_contexts = if added.is_empty() {
            Vec::new()
        } else {
            tokio::task::spawn_blocking(move || {
                let added: Vec<_> = added.into_iter().map(|path| (usize::MAX, path)).collect();
                load_folder_contexts(&added, &server_settings)
            })
            .await
            .unwrap_or_default()
        };
        {
            let mut state = self.state.lock().await;
            state.disk_files.retain(|path, _| !under_removed_root(path));
            // Replacing removed contexts drops their project caches with
            // them; surviving contexts keep theirs, new ones start fresh.
            state
                .folder_contexts
                .retain(|ctx| !removed.iter().any(|root| root == &ctx.root));
            state.folder_contexts.extend(added_contexts);
            state
                .folder_contexts
                .sort_by_key(|ctx| std::cmp::Reverse(ctx.root.as_os_str().len()));
        }
        self.published_uris
            .retain(|path, _| !under_removed_root(path));
        refresh_watchers(self.client.clone(), Arc::clone(&self.state)).await;
        self.rescan().await;
        self.clear(to_clear).await;
        self.request(to_republish).await;
    }

    /// Reload and rescan when package metadata, data, stub, or config files
    /// change. When only sources changed, refresh their index entries.
    async fn watched_files(&mut self, changes: Vec<FileEvent>) {
        let config_paths = custom_config_paths(&*self.state.lock().await);
        let config_or_baseline_changed = changes.iter().any(|change| {
            let path = change.uri.path();
            path.ends_with("ry.toml")
                || path.ends_with(".json")
                || change
                    .uri
                    .to_file_path()
                    .is_ok_and(|path| config_paths.contains(&path))
        });
        let resolution_changed = config_or_baseline_changed
            || changes.iter().any(|change| {
                let path = change.uri.path();
                path.ends_with("DESCRIPTION")
                    || path.ends_with("NAMESPACE")
                    || change.uri.to_file_path().is_ok_and(|path| {
                        path.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("src"))
                            && matches!(
                                path.extension().and_then(|ext| ext.to_str()),
                                Some("c" | "cc" | "cpp" | "cxx")
                            )
                    })
                    || path.ends_with(".rda")
                    || path.ends_with(".RData")
                    || path.ends_with(".rdata")
            });
        let sources: Vec<PathBuf> = changes
            .iter()
            .filter_map(|change| change.uri.to_file_path().ok())
            .filter(|path| ry_workspace::is_source_path(path))
            .collect();
        if !resolution_changed && sources.is_empty() {
            return;
        }
        if config_or_baseline_changed {
            self.reload_folder_contexts().await;
        }

        // Closed files to republish alongside every open document.
        let mut publish: Vec<String> = Vec::new();
        if resolution_changed {
            self.rescan().await;
            publish.extend(self.published_uris.keys().cloned());
        } else {
            for path in sources {
                if self.refresh_disk_entry(&path).await {
                    publish.push(path.to_string_lossy().into_owned());
                }
            }
            if publish.is_empty() {
                return;
            }
            self.refresh_package_contexts(&publish).await;
        }
        publish.extend(self.state.lock().await.docs.paths());
        self.request(publish).await;
    }

    /// Index every source under the workspace folders into `disk_files`
    /// and resolve each package group. The fallback root is never indexed.
    async fn rescan(&mut self) {
        self.indexed = true;
        let roots: Vec<_> = self
            .state
            .lock()
            .await
            .folder_contexts
            .iter()
            .map(|ctx| {
                (
                    ctx.root.clone(),
                    ctx.config_root.clone(),
                    ctx.config.clone(),
                    Arc::clone(&ctx.stubs),
                )
            })
            .collect();
        if roots.is_empty() {
            return;
        }
        let indexed = tokio::task::spawn_blocking(move || {
            let mut all_disk_files: HashMap<String, Arc<SourceFile>> = HashMap::new();
            let mut contexts = Vec::new();
            let mut all_truncated: Vec<(PathBuf, ry_workspace::TruncationReport)> = Vec::new();
            for (root, config_root, config, stubs) in &roots {
                let outcome = crate::index::index_workspace(root, config_root.as_deref(), config);
                if let Some(report) = outcome.truncated {
                    all_truncated.push((root.clone(), report));
                }
                // Each package group resolves against its own root; loose
                // scripts resolve against the folder root.
                let mut paths: Vec<&String> = outcome.files.keys().collect();
                paths.sort();
                let files: Vec<&SourceFile> = paths
                    .iter()
                    .map(|path| outcome.files[*path].as_ref())
                    .collect();
                let mut folder_contexts = HashMap::new();
                for group in ry_workspace::resolve_grouped(&files, config, stubs, root) {
                    match group {
                        Ok(group) => {
                            folder_contexts.insert(group.key, group.context);
                        }
                        Err(error) => tracing::warn!(%error, "workspace resolution degraded"),
                    }
                }
                contexts.push((root.clone(), folder_contexts));
                all_disk_files.extend(outcome.files);
            }
            (all_disk_files, contexts, all_truncated)
        })
        .await;
        let (disk_files, contexts, truncated) = match indexed {
            Ok(indexed) => indexed,
            Err(error) => {
                tracing::warn!(%error, "background workspace index failed");
                return;
            }
        };
        #[cfg(feature = "test-util")]
        crate::test_seam::pause(Seam::Scan).await;
        tracing::info!(
            files = disk_files.len(),
            "background workspace index complete"
        );
        // A cap hit is never silent: a log event per cap plus one
        // user-visible warning per scan.
        for (root, report) in &truncated {
            if report.max_files_hit {
                tracing::warn!(
                    root = %root.display(),
                    cap = "index.max-files",
                    "discovery file-count cap reached; additional R files were not indexed"
                );
            }
            for (path, size) in &report.oversized_files {
                tracing::warn!(
                    root = %root.display(),
                    path = %path.display(),
                    size,
                    cap = "index.max-file-bytes",
                    "file exceeds per-file size cap and was not indexed"
                );
            }
            for dir in &report.depth_pruned_dirs {
                tracing::warn!(
                    root = %root.display(),
                    pruned = %dir.display(),
                    cap = "index.max-depth",
                    "directory depth cap reached; files below were not indexed"
                );
            }
        }
        {
            let mut state = self.state.lock().await;
            state.disk_files = disk_files;
            for ctx in &mut state.folder_contexts {
                if let Some((_, wc)) = contexts.iter().find(|(root, _)| root == &ctx.root) {
                    ctx.workspace_contexts = wc.clone();
                }
            }
        }
        self.notify_degraded_scopes().await;
        if !truncated.is_empty() {
            self.client
                .log_message(
                    MessageType::WARNING,
                    "ry: discovery cap reached; some R files were not indexed. See server logs for details (index.max-files / index.max-file-bytes / index.max-depth).",
                )
                .await;
        }
    }

    /// Refresh one `disk_files` entry with the walk's admission rules and
    /// loader, so a rescan agrees about membership. A missing, inadmissible,
    /// or unreadable file, or one no folder owns, leaves the index. Returns
    /// whether the index changed. An open path is left alone, and a new
    /// entry past `index.max-files` is refused.
    async fn refresh_disk_entry(&self, path: &Path) -> bool {
        let path_string = path.to_string_lossy().into_owned();
        let admission = {
            let state = self.state.lock().await;
            if state.docs.contains(&path_string) {
                return false;
            }
            state
                .folder_context_for_path(&path_string)
                .filter(|_| state.eligibility_for_path(&path_string))
                .map(WalkAdmission::for_folder)
        };
        let budget = admission
            .as_ref()
            .map(|admission| (admission.root.clone(), admission.limits.max_files));
        let read_path = path.to_path_buf();
        let parsed = tokio::task::spawn_blocking(move || {
            admission.filter(|admission| admission.admits(&read_path))?;
            ry_workspace::parse_source_file(&read_path, None)
                .ok()
                .map(Arc::new)
        })
        .await
        .ok()
        .flatten();
        let mut state = self.state.lock().await;
        match (parsed, budget) {
            (Some(file), Some((budget_root, max_files))) => {
                // `index.max-files` is not a path property: refuse a new
                // entry once the owning root is at its budget, first come
                // first served like the walk.
                let over_budget = !state.disk_files.contains_key(&path_string)
                    && state
                        .disk_files
                        .keys()
                        .filter(|existing| {
                            state.entry_consumes_root_budget(Path::new(existing), &budget_root)
                        })
                        .count()
                        >= max_files;
                if over_budget {
                    tracing::warn!(
                        path = %path_string,
                        root = %budget_root.display(),
                        cap = max_files,
                        "discarding per-file disk refresh over index.max-files"
                    );
                    return false;
                }
                state.disk_files.insert(path_string, file);
            }
            // Inadmissible or unreadable: like the walk, never indexed.
            _ => {
                state.disk_files.remove(&path_string);
            }
        }
        true
    }

    /// Re-resolve the package groups owning `paths` after their disk
    /// entries changed. If one fails, a full rescan resolves every group.
    async fn refresh_package_contexts(&mut self, paths: &[String]) {
        let mut groups: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
        {
            let state = self.state.lock().await;
            for path in paths {
                // The fallback context stores no package contexts.
                let Some(ctx) = state.folder_context_for_path(path) else {
                    continue;
                };
                let group = (
                    ctx.root.clone(),
                    ry_workspace::analysis_group_key(Path::new(path)),
                );
                if !groups.contains(&group) {
                    groups.push(group);
                }
            }
        }
        for (folder_root, package_root) in groups {
            if !self
                .refresh_package_context(folder_root, package_root)
                .await
            {
                self.rescan().await;
                return;
            }
        }
    }

    /// Resolve and install one package group's context from the folder's
    /// current disk files (open buffers stay out, as in the scan). Returns
    /// false when resolution failed.
    async fn refresh_package_context(
        &self,
        folder_root: PathBuf,
        package_root: Option<PathBuf>,
    ) -> bool {
        let (mut candidates, config, stubs) = {
            let state = self.state.lock().await;
            let Some(ctx) = state
                .folder_contexts
                .iter()
                .find(|ctx| ctx.root == folder_root)
            else {
                return true;
            };
            let candidates: Vec<(String, Arc<SourceFile>)> = state
                .disk_files
                .iter()
                .filter(|(path, _)| Path::new(path).starts_with(&folder_root))
                .map(|(path, file)| (path.clone(), Arc::clone(file)))
                .collect();
            (candidates, ctx.config.clone(), Arc::clone(&ctx.stubs))
        };
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        let (key, root) = (package_root.clone(), folder_root.clone());
        // Grouping probes the filesystem, so it runs in the blocking task.
        let resolved = tokio::task::spawn_blocking(move || {
            // The full scan's grouping, so a report stays its own group.
            let groups = ry_workspace::group_by_package_root(
                candidates.iter().map(|(path, _)| path.as_str()),
            );
            let files: Vec<&SourceFile> = groups
                .get(&key)
                .into_iter()
                .flatten()
                .map(|&index| candidates[index].1.as_ref())
                .collect();
            ry_workspace::resolve_group(key.as_deref(), files, &config, &stubs, &root)
        })
        .await;
        let context = match resolved {
            Ok(Ok(context)) => context,
            Ok(Err(error)) => {
                tracing::warn!(%error, "incremental workspace resolution degraded");
                return false;
            }
            Err(error) => {
                tracing::warn!(%error, "incremental workspace resolution task failed");
                return false;
            }
        };
        if let Some(ctx) = self
            .state
            .lock()
            .await
            .folder_contexts
            .iter_mut()
            .find(|ctx| ctx.root == folder_root)
        {
            ctx.workspace_contexts.insert(package_root, context);
        }
        self.notify_degraded_scopes().await;
        true
    }

    /// Log each inventory failure the installed contexts newly report.
    async fn notify_degraded_scopes(&self) {
        let newly_degraded = self.state.lock().await.newly_degraded_scopes();
        for (path, reason) in &newly_degraded {
            self.client
                .log_message(MessageType::WARNING, reason.notice(path))
                .await;
        }
    }

    /// Schedule a debounced pass covering `paths`. Every pass publishes all
    /// checked files, so a burst coalesces. With nothing requested, only
    /// dropped URIs are reconciled.
    async fn request(&mut self, paths: impl IntoIterator<Item = String>) {
        self.pending.extend(paths);
        if self.pending.is_empty() {
            self.reconcile(&HashSet::new(), Vec::new()).await;
            return;
        }
        self.deadline = Some(Instant::now() + DEBOUNCE);
    }

    async fn publish_pass(&mut self) {
        self.deadline = None;
        let requested = std::mem::take(&mut self.pending);
        match self.publish_diagnostics().await {
            Some(published) => self.reconcile(&requested, published).await,
            // A document changed while the pass ran; the next pass covers
            // these paths too.
            None => self.request(requested).await,
        }
    }

    /// Check incrementally and publish every checked file, since an edit
    /// can change its cross-file callers. Returns what was published, or
    /// `None` when an edit or queued work made the snapshot stale.
    async fn publish_diagnostics(&self) -> Option<Vec<Published>> {
        // Snapshot under the lock, then check without it so a slow check
        // does not block other requests.
        let (
            generation,
            doc_versions,
            supports_diagnostic_data,
            supports_related_information,
            open_uris,
            collision_sources,
        ) = {
            let mut state = self.state.lock().await;
            // Handlers queue their work under the lock that applied the
            // document change, so an empty queue means nothing pending
            // can change what this snapshot sees.
            if !self.work.is_empty() {
                return None;
            }
            state.diag_generation = state.diag_generation.wrapping_add(1);
            let paths = state.docs.paths();
            (
                state.diag_generation,
                paths
                    .iter()
                    .filter(|path| state.eligibility_for_path(path))
                    .filter_map(|path| Some((path.clone(), state.docs.active(path)?.version)))
                    .collect::<Vec<_>>(),
                state.supports_diagnostic_data,
                state.supports_related_information,
                // Every open path's URIs and the URI of its active document.
                paths
                    .iter()
                    .filter_map(|path| {
                        let active = state.docs.active(path)?.uri.clone();
                        Some((path.clone(), (state.docs.uris(path), active)))
                    })
                    .collect::<HashMap<_, _>>(),
                // Each colliding path's eligible documents, each judged by
                // its own size.
                paths
                    .iter()
                    .filter(|path| state.docs.at(path).len() > 1)
                    .map(|path| {
                        let eligible: HashMap<Url, (String, i32)> = state
                            .docs
                            .at(path)
                            .iter()
                            .filter(|doc| {
                                state.eligibility_for_path_with_len(
                                    path,
                                    Some(doc.text.len() as u64),
                                )
                            })
                            .map(|doc| (doc.uri.clone(), (doc.text.clone(), doc.version)))
                            .collect();
                        (path.clone(), state.context_for_path(path).clone(), eligible)
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let colliding = |path: &str| open_uris.get(path).is_some_and(|(uris, _)| uris.len() > 1);

        let mut open_files = Vec::with_capacity(doc_versions.len());
        // Buffers ry cannot parse publish one RY000 each, as in `ry check`.
        let mut unparsed = Vec::new();
        for (doc_path, version) in &doc_versions {
            match parsed_file(&self.state, doc_path).await {
                Ok((file, _)) => open_files.push((doc_path.clone(), *version, file)),
                Err(Unparsed::Failed { text, error }) => {
                    unparsed.push((doc_path.clone(), *version, text, error));
                }
                Err(Unparsed::Closed) => {}
            }
        }
        // An open buffer owns its path, eligible or not, so its disk twin
        // never contributes. Sorting by path gives the CLI's order, which
        // decides same-name shadowing. The grouping below keeps it.
        let mut project_files: Vec<(Option<PathBuf>, PackageFile)> = {
            let state = self.state.lock().await;
            let disk = state
                .disk_files
                .iter()
                .filter(|(p, _)| !state.docs.contains(p))
                .filter(|(p, _)| state.eligibility_for_path(p))
                .map(|(p, file)| (p.clone(), 0, Arc::clone(file)));
            disk.chain(open_files)
                .map(|file| {
                    let owner = state
                        .folder_context_for_path(&file.0)
                        .map(|ctx| ctx.root.clone());
                    (owner, file)
                })
                .collect()
        };
        project_files.sort_by(|a, b| a.1.0.cmp(&b.1.0));

        // At a colliding display path, each eligible document gets its own
        // RY117 when it has contracts in scope, with its own suppressions
        // and version.
        let mut collision_diagnostics: HashMap<String, HashMap<Url, Vec<LspDiagnostic>>> =
            HashMap::new();
        for (path, ctx, sources) in collision_sources {
            let mut by_uri = HashMap::new();
            if let Some(scope) = ctx.config.annotations.typehint.adopted_scope()
                && !ctx.excludes_path(&path)
            {
                for (uri, (text, version)) in sources {
                    if !text.contains("#|") {
                        continue;
                    }
                    let Some(file) = RParser::new().ok().and_then(|mut parser| {
                        ry_workspace::reports::parse_source(&mut parser, &path, &text).ok()
                    }) else {
                        continue;
                    };
                    if ry_checker::typehint::read_records(&file, &scope).is_empty() {
                        continue;
                    }
                    let native = uri.to_file_path().ok();
                    let warnings = ctx.publish(
                        vec![ry_checker::frontend::ambiguous_typehint_identity(&path)],
                        &path,
                        native.as_deref(),
                        &file,
                        supports_diagnostic_data
                            .then(|| diagnostic_origin(&path, version, generation)),
                        None,
                    );
                    by_uri.insert(uri, warnings);
                }
            }
            collision_diagnostics.insert(path, by_uri);
        }

        // Partition by owning context, then by package group. Each package
        // is its own library scope with its own cache.
        let mut owners: Vec<(Option<PathBuf>, Vec<PackageFile>)> = Vec::new();
        for (owner, file) in project_files {
            match owners.iter_mut().find(|(root, _)| *root == owner) {
                Some((_, files)) => files.push(file),
                None => owners.push((owner, vec![file])),
            }
        }
        let partitions: Vec<(Option<PathBuf>, Option<PathBuf>, Vec<PackageFile>)> = owners
            .into_iter()
            .flat_map(|(owner, files)| {
                let groups =
                    ry_workspace::group_by_package_root(files.iter().map(|file| file.0.as_str()));
                groups
                    .into_iter()
                    .map(|(package_root, indices)| {
                        let files = indices.iter().map(|&index| files[index].clone()).collect();
                        (owner.clone(), package_root, files)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        struct PackageCheckJob {
            ctx: Arc<FolderAnalysisContext>,
            workspace: ry_workspace::WorkspaceContext,
            cache: Arc<Mutex<ProjectCache>>,
            files: Vec<PackageFile>,
        }
        let mut jobs: Vec<PackageCheckJob> = Vec::with_capacity(partitions.len());
        {
            let mut state = self.state.lock().await;
            let mut snapshots: HashMap<Option<PathBuf>, Arc<FolderAnalysisContext>> =
                HashMap::new();
            for (owner, package_root, files) in partitions {
                let ctx = match &owner {
                    Some(root) => state
                        .folder_contexts
                        .iter_mut()
                        .find(|ctx| &ctx.root == root),
                    None => Some(&mut state.fallback),
                };
                let Some(ctx) = ctx else {
                    continue;
                };
                let cache = Arc::clone(ctx.package_caches.entry(package_root.clone()).or_default());
                // A group created after the last index checks against an empty
                // context until the next index resolves it.
                let workspace = ctx
                    .workspace_contexts
                    .get(&package_root)
                    .cloned()
                    .unwrap_or_default();
                let ctx = Arc::clone(
                    snapshots
                        .entry(owner)
                        .or_insert_with(|| Arc::new(ctx.clone())),
                );
                jobs.push(PackageCheckJob {
                    ctx,
                    workspace,
                    cache,
                    files,
                });
            }
        }

        // Every open buffer, eligible or not, hides its disk copy from box.
        let open_paths: HashSet<String> = open_uris.keys().cloned().collect();
        let mut all_results: Vec<(Arc<FolderAnalysisContext>, ProjectCheckResult)> = Vec::new();
        for job in jobs {
            // Colliding paths get their RY117 from the per-URI pass above.
            let adoptable: Vec<Arc<SourceFile>> = job
                .files
                .iter()
                .filter(|(path, _, _)| !colliding(path))
                .map(|(_, _, file)| Arc::clone(file))
                .collect();
            let adopted = ry_checker::frontend::adopted_records(&adoptable, &job.ctx.config);
            let mut project = job.cache.lock().await;
            let mut result = project.check_with_workspace(
                job.files,
                Arc::clone(&job.ctx.stubs),
                job.workspace,
                adopted.records,
                &open_paths,
            );
            for diagnostic in adopted.diagnostics {
                if let Some((_, diagnostics)) = result
                    .diagnostics
                    .iter_mut()
                    .find(|(path, _)| *path == diagnostic.path)
                {
                    diagnostics.push(diagnostic);
                }
            }
            all_results.push((job.ctx, result));
        }

        // An edit during the check invalidates the whole result.
        if self.state.lock().await.diag_generation != generation {
            return None;
        }

        let diagnostic_versions: HashMap<_, _> = doc_versions.into_iter().collect();

        let publication_uris = |path: &str| {
            open_uris
                .get(path)
                .map_or_else(|| vec![path_to_uri(path)], |(uris, _)| uris.clone())
        };
        // Post-process each file through the context it was checked under.
        // `None` marks a colliding path that was not checked because its
        // active document is over a cap. Its eligible siblings still own
        // their RY117.
        let mut outgoing: Vec<(String, Option<Vec<LspDiagnostic>>)> = Vec::new();
        for (ctx, result) in all_results {
            let ProjectCheckResult {
                diagnostics: per_file,
                files: checked_files,
            } = result;
            for (diagnostic_path, diagnostics) in per_file {
                let Some(file) = checked_files.get(&diagnostic_path) else {
                    continue;
                };
                if ctx.excludes_path(&diagnostic_path) {
                    continue;
                }
                let uris = publication_uris(&diagnostic_path);
                // Related items share the diagnostic's display path, so they
                // have a location only when that path names one document.
                let related_uri = uris
                    .first()
                    .filter(|_| uris.len() == 1 && supports_related_information);
                let diagnostics = ctx.publish(
                    diagnostics,
                    &diagnostic_path,
                    Some(Path::new(&diagnostic_path)),
                    file,
                    diagnostic_versions
                        .get(&diagnostic_path)
                        .filter(|_| supports_diagnostic_data)
                        .map(|version| diagnostic_origin(&diagnostic_path, *version, generation)),
                    related_uri,
                );
                outgoing.push((diagnostic_path, Some(diagnostics)));
            }
        }
        if !unparsed.is_empty() {
            // Snapshot the contexts so post-processing runs without the lock,
            // as it does for checked files.
            let contexts: Vec<FolderAnalysisContext> = {
                let state = self.state.lock().await;
                unparsed
                    .iter()
                    .map(|(path, ..)| state.context_for_path(path).clone())
                    .collect()
            };
            for ((path, version, text, error), ctx) in unparsed.into_iter().zip(contexts) {
                if ctx.excludes_path(&path) {
                    continue;
                }
                let origin =
                    supports_diagnostic_data.then(|| diagnostic_origin(&path, version, generation));
                let diagnostics = ctx.publish_parse_failure(&path, &text, error, origin);
                outgoing.push((path, Some(diagnostics)));
            }
        }
        for path in collision_diagnostics.keys() {
            if !outgoing.iter().any(|(checked, _)| checked == path) {
                outgoing.push((path.clone(), None));
            }
        }

        let mut published: Vec<Published> = Vec::new();
        for (path, diagnostics) in outgoing {
            let uris = publication_uris(&path);
            let active = open_uris.get(&path).map(|(_, active)| active);
            let checked = diagnostics.is_some();
            let mut diagnostics = diagnostics.unwrap_or_default();
            let mut warnings = collision_diagnostics.remove(&path).unwrap_or_default();
            let mut non_empty = HashSet::new();
            for uri in &uris {
                // The active document owns the check results. Other URIs at
                // the path get only their own RY117, never its findings.
                let owner = active.is_none_or(|active| active == uri);
                let mut for_uri = if owner {
                    std::mem::take(&mut diagnostics)
                } else {
                    Vec::new()
                };
                for_uri.extend(warnings.remove(uri).unwrap_or_default());
                if !for_uri.is_empty() {
                    non_empty.insert(uri.clone());
                }
                let version = diagnostic_versions
                    .get(&path)
                    .copied()
                    .filter(|_| owner && checked);
                self.client
                    .publish_diagnostics(uri.clone(), for_uri, version)
                    .await;
            }
            published.push((path, uris.into_iter().collect(), non_empty));
        }
        Some(published)
    }

    /// Clear every URI that should no longer show diagnostics: ones this
    /// pass stopped publishing to, requested paths that are not analyzed
    /// (acknowledging a document opened into a disabled folder or past a
    /// cap), and tracked paths that are no longer eligible, open, or indexed.
    async fn reconcile(&mut self, requested: &HashSet<String>, published: Vec<Published>) {
        let mut stale: HashSet<Url> = HashSet::new();
        for (path, destinations, non_empty) in published {
            let previous = self.published_uris.remove(&path).unwrap_or_default();
            stale.extend(previous.difference(&destinations).cloned());
            if !non_empty.is_empty() {
                self.published_uris.insert(path, non_empty);
            }
        }
        {
            let state = self.state.lock().await;
            for path in requested {
                if !state.any_open_source_eligible(path) {
                    stale.extend(state.docs.uris(path));
                }
            }
            let dropped: Vec<String> = self
                .published_uris
                .keys()
                .filter(|path| {
                    !state.any_open_source_eligible(path)
                        || (!state.docs.contains(path)
                            && !state.disk_files.contains_key(path.as_str()))
                })
                .cloned()
                .collect();
            for path in dropped {
                stale.extend(self.published_uris.remove(&path).unwrap_or_default());
            }
        }
        self.clear(stale).await;
    }

    async fn clear(&self, uris: impl IntoIterator<Item = Url>) {
        for uri in uris {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }
}

/// A folder's walk-admission inputs, snapshotted for one refresh.
struct WalkAdmission {
    root: PathBuf,
    anchor: PathBuf,
    excludes: ry_config::Excludes,
    include_build_ignored: Vec<String>,
    limits: ry_workspace::DiscoveryLimits,
    check_test_fixtures: bool,
}

impl WalkAdmission {
    fn for_folder(ctx: &FolderAnalysisContext) -> Self {
        WalkAdmission {
            root: ctx.root.clone(),
            anchor: ctx.config_root.clone().unwrap_or_else(|| ctx.root.clone()),
            excludes: ctx.excludes.clone(),
            include_build_ignored: ctx.config.include_build_ignored.clone(),
            limits: ry_workspace::DiscoveryLimits::from_config(&ctx.config),
            check_test_fixtures: ctx.config.check_test_fixtures,
        }
    }

    /// Whether the walk from this folder's root would admit `path`. The
    /// `index.max-files` count is enforced at commit instead.
    fn admits(&self, path: &Path) -> bool {
        ry_workspace::is_single_file_walk_admitted(
            path,
            &self.root,
            Some(&self.anchor),
            &self.excludes,
            &self.include_build_ignored,
            &self.limits,
            self.check_test_fixtures,
        )
    }
}
