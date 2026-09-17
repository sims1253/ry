//! LSP backend: `Backend`, `State`, the `LanguageServer` impl, and the
//! document cache / debounce machinery.
//!
//! All request handlers read the cached parse (`State::parsed`) and the
//! cached assignment hints (`State::hints`); diagnostics are debounced
//! via `schedule_diagnostics`.

mod handlers;

use crate::diagnostics::{
    diagnostic_origin, diagnostic_to_lsp, diagnostic_to_lsp_with_source, make_ignore_action,
    make_ignore_file_action,
};
use crate::hints::collect_inlay_hints;
use crate::positions::{byte_offset_to_point, position_to_byte_offset};
use crate::settings::{FolderSettings, ServerSettings};

use ry_checker::Project;
use ry_core::{RParser, SourceFile};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::Diagnostic as LspDiagnostic;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

/// Counts baseline file reads performed by `load_folder_baseline`, the
/// only baseline disk-read site in the LSP. Exposed via
/// [`baseline_disk_reads`] so integration tests can assert hot-path I/O
/// is absent rather than infer it from timing. Test-util builds only
/// (#170): the counter never compiles into the production binary.
#[cfg(feature = "test-util")]
static BASELINE_DISK_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Number of baseline file reads since process start. A publish/inlay-hint
/// that does not change this value performs zero baseline disk I/O.
#[cfg(feature = "test-util")]
pub fn baseline_disk_reads() -> usize {
    BASELINE_DISK_READS.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone)]
pub(super) struct Backend {
    pub(super) client: Client,
    pub(super) state: Arc<Mutex<State>>,
}

struct CachedHints {
    version: i32,
    stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    hints: Vec<InlayHint>,
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
    /// request handlers, invalidated by `update_doc`. Each cache miss
    /// constructs a parser; only the result is cached.
    parsed: HashMap<String, (i32, Arc<SourceFile>)>,
    /// Hints belong to one document version and loaded stub snapshot.
    hints: HashMap<String, CachedHints>,
    /// Workspace-wide debounce generation; see `schedule_diagnostics`.
    diag_generation: u64,
    /// Paths with a scheduled-but-unpublished diagnostics request; see
    /// `schedule_diagnostics`. The task holding the latest generation
    /// drains the whole set, so a burst of scheduled URIs publishes
    /// together instead of all but the last aborting as stale (#489).
    pending_diag_paths: HashSet<String>,
    /// Paths whose last publication carried diagnostics. Each publish
    /// pass reconciles this set — a URI that stopped receiving
    /// publications (its folder was disabled, discovery now excludes
    /// it, or a rescan dropped the closed file from the index) is
    /// cleared with an empty publication so stale squiggles cannot
    /// linger in the editor (#489).
    published_paths: HashSet<String>,
    /// Index generation stamp, bumped each time `spawn_background_index`
    /// starts so results from a prior folder set are discarded. The
    /// background task captures the generation at dispatch and checks it
    /// before writing.
    index_generation: u64,
    /// Files opened during initialization wait for the first workspace context.
    initial_index_pending: bool,
    /// Runtime stubs loaded from the workspace's `ry.toml`. Kept in state so
    /// every rebuilt Project and single-file scope check sees the same data.
    user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Persistent multi-file checker used only by diagnostics. Its own mutex
    /// keeps project checks serialized without holding the document-state
    /// lock used by latency-sensitive LSP requests.
    project: Arc<Mutex<ProjectCache>>,
    /// Counts every actual parse (`RParser::parse`) performed by
    /// `parsed_file` -- i.e. every cache MISS. The cache acceptance test
    /// asserts that editing one file in a multi-file workspace parses
    /// only that file, so this counter must NOT rise for cache hits.
    #[cfg(test)]
    pub(super) parse_count: Arc<std::sync::atomic::AtomicUsize>,

    // --- settings channel ---
    /// The workspace root directory (from `root_uri`), used for
    /// `ry.toml` discovery and relative path resolution.
    root: Option<PathBuf>,
    /// The full `ry-config::Config` loaded from `ry.toml` at the workspace
    /// root, stored so `publish_diagnostics` never re-reads the file.
    file_config: ry_config::Config,
    /// Directory of the root-level fallback config's `ry.toml` (`None`
    /// when defaults are in use). Anchors the fallback's config-relative
    /// `exclude` patterns and baseline keys the way `ry check` anchors
    /// them at the config directory (#493).
    root_config_dir: Option<PathBuf>,
    /// Root-level baseline cached at initialize so the fallback publish
    /// path performs no disk access.
    root_baseline: Option<ry_config::Baseline>,
    /// Root-level filter, confidence threshold, and excludes, precomputed
    /// from `file_config` and the root `folder_settings`.
    ///
    /// A document outside every folder root must be filtered by root-level
    /// config.
    root_filter: ry_checker::SeverityFilter,
    root_min_confidence: Option<ry_checker::Confidence>,
    root_excludes: ry_config::Excludes,
    /// Editor-supplied per-folder settings, received via
    /// `initializationOptions`, `workspace/configuration`, or
    /// `didChangeConfiguration`.
    folder_settings: FolderSettings,
    /// The full settings envelope received at initialize; retained so
    /// dynamically added folders build through the same
    /// `build_folder_contexts` path as initial ones.
    server_settings: ServerSettings,
    /// Whether the client supports `workspace/configuration` pull (then
    /// `didChangeConfiguration` re-pulls instead of parsing the payload).
    supports_workspace_configuration: bool,
    /// Whether workspace edits can carry the document version they modify.
    supports_document_changes: bool,
    /// Whether the client preserves diagnostic data in code-action requests.
    supports_diagnostic_data: bool,
    /// Whether the client supports dynamic registration of
    /// `workspace/didChangeWatchedFiles`.
    supports_did_change_watched_files: bool,
    supports_relative_patterns: bool,
    watcher_paths: Arc<Mutex<Option<Vec<PathBuf>>>>,

    // --- multi-root workspace folders ---
    /// Per-root analysis contexts, ordered by root path length descending
    /// for longest-prefix ownership. Each context owns its folder's
    /// project cache (see [`FolderAnalysisContext::project_cache`]).
    folder_contexts: Vec<FolderAnalysisContext>,
    /// On-disk `.R`/`.r` files discovered by the background indexer,
    /// keyed by absolute path. Open documents shadow these.
    disk_files: HashMap<String, Arc<SourceFile>>,
    /// Version-stamped tree-sitter trees for incremental parsing: a tree
    /// is stored or served only when its recorded version still matches
    /// the current document version, so no cached tree can ever be
    /// served for a different document generation.
    trees: HashMap<String, (i32, ry_core::Tree)>,
}

/// One per-folder analysis context. Analysis channels resolve config,
/// editor settings, local typesheds, and package metadata through the
/// owning folder, so two roots defining the same package differently
/// never collide.
#[derive(Clone, Default)]
pub(super) struct FolderAnalysisContext {
    /// The workspace folder root directory.
    pub root: PathBuf,
    /// Directory of the `ry.toml` the effective config was loaded from;
    /// `None` when defaults are in use. Every config-relative resolution
    /// — `exclude` patterns, `include-build-ignored`, and baseline key
    /// normalization — anchors here (the CLI's config root) instead of
    /// the workspace folder, so an inherited or external config keeps
    /// its own path anchor. Equals `root` whenever the config lives in
    /// the folder itself, so only configs above or outside the folder
    /// change behavior (#493).
    pub config_root: Option<PathBuf>,
    /// Effective `ry.toml` config: loaded from directory discovery or
    /// the editor `configuration` override resolved relative to `root`.
    pub config: ry_config::Config,
    /// Editor-supplied per-folder settings.
    pub folder_settings: FolderSettings,
    /// Local typeshed stubs loaded from this folder's `ry.toml`.
    pub stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Workspace resolution context for package metadata.
    pub workspace_context: Option<ry_workspace::WorkspaceContext>,
    /// The baseline loaded from `ry.toml`/editor settings, cached during
    /// context construction so the publish path performs no disk access.
    pub baseline: Option<ry_config::Baseline>,
    /// Severity filter compiled once during context construction.
    pub filter: ry_checker::SeverityFilter,
    /// Precomputed minimum confidence threshold.
    pub min_confidence: Option<ry_checker::Confidence>,
    /// Precompiled exclude glob patterns.
    pub excludes: ry_config::Excludes,
    /// This folder's project cache for isolated checking. Each workspace
    /// folder gets its own `ProjectCache` so two roots defining the same
    /// package differently never collide. Shared via `Arc` and carried
    /// across context rebuilds so incremental check state survives a
    /// config reload.
    pub project_cache: Arc<Mutex<ProjectCache>>,
}

/// Compile the filter, min_confidence, and excludes for a folder from its
/// config and settings. The folder config is both the exclude source and
/// the severity fallback.
fn compute_folder_filter(
    config: &ry_config::Config,
    folder_settings: &FolderSettings,
) -> (
    ry_checker::SeverityFilter,
    Option<ry_checker::Confidence>,
    ry_config::Excludes,
) {
    let lint = &folder_settings.lint;
    let error = lint.error.clone().unwrap_or_else(|| config.error.clone());
    let warn = lint.warn.clone().unwrap_or_else(|| config.warn.clone());
    let ignore = lint.ignore.clone().unwrap_or_else(|| config.ignore.clone());
    let mut filter = ry_checker::build_filter(&error, &warn, &ignore);
    let select = lint.select.as_ref().or(config.select.as_ref());
    let extend_select = lint.extend_select.as_ref().unwrap_or(&config.extend_select);
    if let Some(select) = select {
        filter.begin_selection();
        for rule in select {
            filter.add_select(rule);
        }
    }
    for rule in extend_select {
        filter.add_extend_select(rule);
    }

    let min_confidence = folder_settings
        .min_confidence
        .as_ref()
        .and_then(|s| match s.as_str() {
            "low" => Some(ry_checker::Confidence::Low),
            "medium" => Some(ry_checker::Confidence::Medium),
            "high" => Some(ry_checker::Confidence::High),
            _ => None,
        });

    let excludes = ry_config::Excludes::from_config(config);

    (filter, min_confidence, excludes)
}

/// Recompute the cached filter / min_confidence / excludes for every
/// folder context and the root-level fallback from their installed
/// `folder_settings`. Never called from `publish_diagnostics`, which
/// borrows these precomputed values instead of recompiling them.
fn refresh_cached_folder_filters(state: &mut State) {
    for ctx in &mut state.folder_contexts {
        let (filter, min_confidence, excludes) =
            compute_folder_filter(&ctx.config, &ctx.folder_settings);
        ctx.filter = filter;
        ctx.min_confidence = min_confidence;
        ctx.excludes = excludes;
    }
    let (root_filter, root_min_confidence, root_excludes) =
        compute_folder_filter(&state.file_config, &state.folder_settings);
    state.root_filter = root_filter;
    state.root_min_confidence = root_min_confidence;
    state.root_excludes = root_excludes;
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

/// Partitioned project files with the owning folder context (if any).
type FolderPartition = (
    Option<FolderAnalysisContext>,
    Vec<(String, i32, Arc<SourceFile>)>,
);

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
        let order: Vec<String> = files.iter().map(|(path, _, _)| path.clone()).collect();
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
        // `update_file` appends re-added paths at the end (after
        // did_close removed them), so insertion history would decide
        // which same-named definition wins. The incoming sequence is
        // the canonical one; enforce it regardless of history (#490).
        self.project.reorder_files(&order);
        ProjectCheckResult {
            diagnostics: self.project.check_incremental(),
            files: checked_files,
        }
    }
}

impl State {
    /// Return the cached parse for `path` when its version matches the
    /// latest recorded version, else `None`. Pure cache read -- does
    /// NOT parse.
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

    /// Return the cached tree-sitter `Tree` for `path`, but only when
    /// its recorded version matches the current document version (the
    /// read half of `store_tree`'s write-side invariant).
    fn tree_for(&self, path: &str) -> Option<ry_core::Tree> {
        let current_version = self.versions.get(path).copied()?;
        let (tree_version, tree) = self.trees.get(path)?;
        if *tree_version == current_version {
            Some(tree.clone())
        } else {
            None
        }
    }

    /// Store a tree-sitter `Tree` for `path`, tagged with `version`.
    /// The entry is written only if `version` still matches the current
    /// document version, so a parse superseded by a concurrent edit is
    /// dropped and no cached tree is ever served for a different
    /// document generation.
    fn store_tree(&mut self, path: &str, version: i32, tree: ry_core::Tree) {
        if self.versions.get(path).copied() == Some(version) {
            self.trees.insert(path.to_string(), (version, tree));
        }
    }

    /// Drop the cached parse and hints for `path`, mirroring the
    /// cache-invalidation half of `Backend::update_doc`. Test-only;
    /// lets the cache acceptance test simulate a `did_change` on a bare
    /// `State` without a `tower_lsp::Client`.
    #[cfg(test)]
    pub(super) fn invalidate_parse(&mut self, path: &str) {
        self.parsed.remove(path);
        self.hints.remove(path);
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

    // --- effective config computation ---

    /// Test helper: the effective `SeverityFilter` from editor settings
    /// merged over the root `ry.toml`, through the same
    /// [`compute_folder_filter`] the production paths use.
    #[cfg(test)]
    pub(super) fn effective_filter(&self) -> ry_checker::SeverityFilter {
        compute_folder_filter(&self.file_config, &self.folder_settings).0
    }

    /// Find the owning [`FolderAnalysisContext`] for a document path
    /// using longest-prefix matching against folder context roots.
    pub(super) fn folder_context_for_path(&self, doc_path: &str) -> Option<&FolderAnalysisContext> {
        let path = std::path::Path::new(doc_path);
        self.folder_contexts
            .iter()
            .find(|ctx| path.starts_with(&ctx.root))
    }

    /// Whether the server should analyze and publish diagnostics for
    /// `doc_path`: a folder set to `enable: false` is skipped entirely;
    /// otherwise eligibility follows the owning folder's discovery rules —
    /// `exclude` patterns plus the `index.max-file-bytes` and
    /// `index.max-depth` caps the background indexer enforces, through the
    /// shared [`ry_workspace::is_file_eligible_with_limits`] policy, so an
    /// opened buffer cannot leak definitions the closed index omitted into
    /// project-wide state (#488).
    ///
    /// The size gate measures the open buffer's text length, never
    /// `fs::metadata`: unsaved pasted content can cross the boundary
    /// without touching disk. Closed paths carry no buffer; they entered
    /// through the capped walker, so only the exclude and depth thirds
    /// apply to them. Depth counts the containing directory's components
    /// relative to the folder root — walk depth is not a path property,
    /// so it is recomputed here.
    fn eligibility_for_path(&self, doc_path: &str) -> bool {
        let path = std::path::Path::new(doc_path);
        // The open buffer's byte length, when the path is open. Closed
        // paths pass `None` and skip the size gate (see above).
        let content_len = self.docs.get(doc_path).map(|text| text.len() as u64);
        if let Some(ctx) = self.folder_context_for_path(doc_path) {
            if ctx.folder_settings.enable == Some(false) {
                return false;
            }
            // Exclude patterns are relative to the originating
            // `ry.toml`'s directory, the anchor `ry check` uses (#493).
            let anchor = ctx.config_root.as_deref().unwrap_or(&ctx.root);
            return ry_workspace::is_file_eligible_with_limits(
                path,
                &ctx.root,
                Some(anchor),
                &ctx.excludes,
                &ry_workspace::DiscoveryLimits::from_config(&ctx.config),
                content_len,
            );
        }
        if self.folder_settings.enable == Some(false) {
            return false;
        }
        // No folder owns the path. Fall back to the server root only
        // when the path is actually inside it. After a folder removal,
        // files under the removed root must not remain eligible via
        // this fallback.
        match &self.root {
            Some(root) if path.starts_with(root) => {
                let anchor = self.root_config_dir.as_deref().unwrap_or(root);
                ry_workspace::is_file_eligible_with_limits(
                    path,
                    root,
                    Some(anchor),
                    &self.root_excludes,
                    &ry_workspace::DiscoveryLimits::from_config(&self.file_config),
                    content_len,
                )
            }
            _ => true,
        }
    }
}

impl Backend {
    /// Reload off the runtime; retain the last valid config on parse errors.
    async fn reload_folder_contexts(&self) {
        let contexts = self.state.lock().await.folder_contexts.clone();
        let contexts = match tokio::task::spawn_blocking(move || {
            contexts
                .iter()
                .map(rebuild_folder_context)
                .collect::<Vec<_>>()
        })
        .await
        {
            Ok(contexts) => contexts,
            Err(error) => {
                tracing::warn!(%error, "folder context reload task failed; retaining previous contexts");
                return;
            }
        };
        let mut state = self.state.lock().await;
        // Reuse the root context's loaded stubs and filters for fallback checks.
        if let Some(ctx) = contexts
            .iter()
            .find(|ctx| state.root.as_deref() == Some(ctx.root.as_path()))
        {
            state.file_config = ctx.config.clone();
            state.root_config_dir = ctx.config_root.clone();
            state.user_stubs = ctx.stubs.clone();
            state.root_baseline = ctx.baseline.clone();
            state.root_filter = ctx.filter.clone();
            state.root_min_confidence = ctx.min_confidence;
            state.root_excludes = ctx.excludes.clone();
        }
        state.folder_contexts = contexts;
        tracing::info!("workspace config/baseline reloaded");
    }

    async fn refresh_watchers(&self) {
        let registered = Arc::clone(&self.state.lock().await.watcher_paths);
        // Serialize replacement registrations without holding the document lock.
        let mut registered = registered.lock().await;
        let (paths, relative) = {
            let state = self.state.lock().await;
            if !state.supports_did_change_watched_files {
                return;
            }
            (
                custom_config_paths(&state),
                state.supports_relative_patterns,
            )
        };
        if registered.as_ref() == Some(&paths) {
            return;
        }
        if registered.is_some() {
            if let Err(error) = self
                .client
                .unregister_capability(vec![Unregistration {
                    id: "ry-workspace-watcher".into(),
                    method: "workspace/didChangeWatchedFiles".into(),
                }])
                .await
            {
                tracing::warn!(%error, "failed to unregister workspace watcher");
                return;
            }
            *registered = None;
        }
        let mut watchers = vec![
            serde_json::json!({"globPattern": "**/ry.toml"}),
            serde_json::json!({"globPattern": "**/DESCRIPTION"}),
            serde_json::json!({"globPattern": "**/NAMESPACE"}),
            serde_json::json!({"globPattern": "**/src/*.{c,cc,cpp,cxx}"}),
            serde_json::json!({"globPattern": "**/*.{rda,RData,rdata,json}"}),
        ];
        for path in &paths {
            let pattern = if relative {
                path.parent()
                    .zip(path.file_name())
                    .and_then(|(parent, name)| {
                        Url::from_directory_path(parent).ok().map(|base| serde_json::json!({
                        "baseUri": base, "pattern": escape_watch_path(&name.to_string_lossy())
                    }))
                    })
            } else {
                None
            }
            .unwrap_or_else(|| {
                // String patterns are best effort: clients may only watch
                // workspace files. External watches need RelativePattern.
                let path = path.to_string_lossy();
                let path = if cfg!(windows) {
                    path.replace('\\', "/")
                } else {
                    path.into_owned()
                };
                serde_json::json!(escape_watch_path(&path))
            });
            watchers.push(serde_json::json!({"globPattern": pattern}));
        }
        let registration = Registration {
            id: "ry-workspace-watcher".into(),
            method: "workspace/didChangeWatchedFiles".into(),
            register_options: Some(serde_json::json!({"watchers": watchers})),
        };
        match self.client.register_capability(vec![registration]).await {
            Ok(()) => *registered = Some(paths),
            Err(error) => tracing::warn!(%error, "failed to register workspace watcher"),
        }
    }

    /// Apply a single incremental text change. A ranged change is spliced
    /// into the old text and drives a tree-sitter `InputEdit` so the
    /// reparse is incremental; everything else replaces the document
    /// wholesale.
    async fn apply_incremental_change(
        &self,
        path: &str,
        change: TextDocumentContentChangeEvent,
        version: i32,
    ) -> bool {
        if let Some(range) = change.range {
            let (old_text, old_tree) = {
                let state = self.state.lock().await;
                let old = state.docs.get(path).cloned();
                (old, state.tree_for(path))
            };

            if let Some(old_text) = old_text {
                // Invalid UTF-16 endpoints (including a position inside an
                // astral surrogate pair) cannot describe a byte splice.
                // Ignore the malformed event rather than clamping it and
                // corrupting the document.
                let Some((start_byte, end_byte)) = range_byte_span(&old_text, range) else {
                    tracing::error!(
                        ?range,
                        "invalid UTF-16 range in document change; server and client text will desynchronize until a full sync is received"
                    );
                    return false;
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

                let mut tree_mut = old_tree;
                if let Some(ref mut tree) = tree_mut {
                    tree.edit(&edit);
                }
                let mut state = self.state.lock().await;
                if let Some(tree) = tree_mut {
                    state.store_tree(path, version, tree);
                } else {
                    state.trees.remove(path);
                }
                return true;
            }
        }
        // Full replacement: no range, or no old text to splice into. Drop
        // any stale tree so the next parse is a full parse.
        {
            let mut state = self.state.lock().await;
            state.trees.remove(path);
        }
        self.update_doc(path.to_string(), change.text, version)
            .await;
        true
    }

    async fn update_doc(&self, path: String, text: String, version: i32) {
        let mut state = self.state.lock().await;
        state.docs.insert(path.clone(), text);
        state.versions.insert(path.clone(), version);
        // Invalidate the cached parse and hints; the next read repopulates.
        state.parsed.remove(&path);
        state.hints.remove(&path);
    }

    /// Return the current AST for `path` together with the exact source
    /// text it was parsed from: handlers use the text for byte-offset /
    /// UTF-16 conversions that must match the AST's span offsets, so a
    /// concurrent `didChange` racing the parse can never yield a stale
    /// text applied to a fresher AST (or vice versa). The parse cache is
    /// read and repopulated under the state lock; parsing happens outside
    /// it. Returns `None` when the path is not open or parsing fails.
    async fn parsed_file(&self, path: &str) -> Option<(Arc<SourceFile>, String)> {
        loop {
            // One guard reads the parse cache, the document text/version,
            // and the old tree as one snapshot; parsing happens outside
            // the lock. Clone the old tree only on a cache miss.
            let (text, version, old_tree) = {
                let state = self.state.lock().await;
                if let (Some(text), Some(version)) =
                    (state.docs.get(path), state.versions.get(path))
                {
                    // Fast path: version-matched cache hit.
                    if let Some(file) = state.cached_parse(path) {
                        return Some((file, text.clone()));
                    }
                    (text.clone(), *version, state.tree_for(path))
                } else {
                    return None;
                }
            };
            // Incremental reparse when a version-matched old tree exists.
            // Test-only scheduling barrier: when armed, the parse pauses
            // here — after reading text/version/tree, before parsing — so a
            // test can force the interleaving:
            //   1. parse version N starts (we are here)
            //   2. didChange installs N+1
            //   3. parse N finishes (test releases the barrier)
            //   4. stale result is rejected by store_tree / record_parse
            //   5. the retry loop parses the current version N+1 fresh
            // The seam controls scheduling only; compiled under the
            // `test-util` feature, it is absent from production builds
            // (#170) and costs nothing when not armed.
            #[cfg(feature = "test-util")]
            crate::test_seam::maybe_pause().await;
            let mut parser = RParser::new().ok()?;
            let (parsed, new_tree) = parser
                .parse_with_tree(path, &text, old_tree.as_ref())
                .ok()?;
            {
                let mut state = self.state.lock().await;
                state.store_tree(path, version, new_tree);
            }
            let file = Arc::new(parsed);
            let mut state = self.state.lock().await;
            // If an edit landed while parsing, retry against the new version
            // instead of returning an AST already known to be stale.
            if state.record_parse(path, version, Arc::clone(&file)) {
                return Some((file, text));
            }
        }
    }

    /// Return hints from one current parse and its assignment-site inference.
    async fn hints_for(&self, path: &str) -> Option<Vec<InlayHint>> {
        {
            let state = self.state.lock().await;
            let stubs = state
                .folder_context_for_path(path)
                .map(|ctx| &ctx.stubs)
                .unwrap_or(&state.user_stubs);
            if let Some(version) = state.versions.get(path).copied()
                && let Some(cached) = state.hints.get(path)
                && cached.version == version
                && Arc::ptr_eq(&cached.stubs, stubs)
            {
                return Some(cached.hints.clone());
            }
        }
        let (file, text) = self.parsed_file(path).await?;
        let mut checker = ry_checker::Checker::new(path);
        let user_stubs = {
            let state = self.state.lock().await;
            state
                .folder_context_for_path(path)
                .map(|ctx| Arc::clone(&ctx.stubs))
                .unwrap_or_else(|| Arc::clone(&state.user_stubs))
        };
        checker.set_user_stubs(Arc::clone(&user_stubs));
        checker.enable_assignment_capture();
        checker.check(&file);
        let hints = collect_inlay_hints(&checker.take_assignment_types(), &text);
        let mut state = self.state.lock().await;
        let current_stubs = state
            .folder_context_for_path(path)
            .map(|ctx| &ctx.stubs)
            .unwrap_or(&state.user_stubs);
        if !Arc::ptr_eq(current_stubs, &user_stubs) {
            return None;
        }
        let version = state
            .parsed
            .get(path)
            .and_then(|(cached_version, cached)| {
                (Arc::ptr_eq(cached, &file)
                    && state.versions.get(path).copied() == Some(*cached_version))
                .then_some(*cached_version)
            })?;
        state.hints.insert(
            path.to_string(),
            CachedHints {
                version,
                stubs: user_stubs,
                hints: hints.clone(),
            },
        );
        Some(hints)
    }

    /// Pull `ry` settings per folder scope via `workspace/configuration`:
    /// one item per folder root plus a final root-scoped item for the
    /// server-wide fallback. Results install into the matching contexts
    /// (by index) and feed [`refresh_cached_folder_filters`]. Shared by
    /// `initialized` and `did_change_configuration` so the paths cannot
    /// drift.
    async fn pull_folder_settings(&self) {
        // Built under the lock, then sent without it.
        let items: Vec<ConfigurationItem> = {
            let state = self.state.lock().await;
            state
                .folder_contexts
                .iter()
                .map(|ctx| ConfigurationItem {
                    scope_uri: Url::from_file_path(&ctx.root).ok(),
                    section: Some("ry".to_string()),
                })
                .chain(std::iter::once(ConfigurationItem {
                    scope_uri: state
                        .root
                        .as_ref()
                        .and_then(|p| Url::from_file_path(p).ok()),
                    section: Some("ry".to_string()),
                }))
                .collect()
        };

        let values = match self.client.configuration(items).await {
            Ok(values) => values,
            Err(e) => {
                tracing::warn!("workspace/configuration pull failed: {e}");
                return;
            }
        };

        let mut state = self.state.lock().await;
        let folder_count = state.folder_contexts.len();
        for (idx, ctx) in state.folder_contexts.iter_mut().enumerate() {
            if let Some(value) = values.get(idx)
                && let Ok(settings) = serde_json::from_value::<FolderSettings>(value.clone())
            {
                ctx.folder_settings = settings;
            }
        }
        if let Some(value) = values.get(folder_count)
            && let Ok(settings) = serde_json::from_value::<FolderSettings>(value.clone())
        {
            state.folder_settings = settings.clone();
            state.server_settings.global_settings = settings;
        }
        refresh_cached_folder_filters(&mut state);
    }

    /// Incrementally update the project and publish diagnostics for every
    /// open document. Publishing all files is required because an edit to a
    /// function definition can change diagnostics in its cross-file callers.
    ///
    /// `requested` is the set of paths the debounce drained for this
    /// generation. A requested path that is no longer eligible still gets
    /// an immediate empty publication (the didOpen acknowledgment for a
    /// document opened into a disabled folder, an excluded path, or over
    /// a size/depth cap); every other URI that drops out of the check is
    /// reconciled at the end of the pass through
    /// [`Backend::clear_dropped_diagnostics`] (#489).
    async fn publish_diagnostics(&self, requested: HashSet<String>, generation: u64) {
        // Snapshot the open docs, each requested path's eligibility, and
        // the capability flags under the lock, then drop it before
        // checking so a slow check doesn't block other LSP requests
        // (e.g. didOpen of a second file). Only eligible documents'
        // versions are snapshotted.
        let (doc_versions, requested_ineligible, supports_diagnostic_data) = {
            let state = self.state.lock().await;
            if state.initial_index_pending {
                return;
            }
            (
                state
                    .docs
                    .keys()
                    .filter(|p| state.eligibility_for_path(p))
                    .filter_map(|p| {
                        state
                            .versions
                            .get(p)
                            .copied()
                            .map(|version| (p.clone(), version))
                    })
                    .collect::<Vec<_>>(),
                requested
                    .iter()
                    .filter(|p| !state.eligibility_for_path(p))
                    .cloned()
                    .collect::<Vec<_>>(),
                state.supports_diagnostic_data,
            )
        };
        for path in &requested_ineligible {
            self.client
                .publish_diagnostics(path_to_uri(path), Vec::new(), None)
                .await;
        }
        if !requested_ineligible.is_empty() {
            let mut state = self.state.lock().await;
            for path in &requested_ineligible {
                state.published_paths.remove(path);
            }
        }

        let mut open_files = Vec::with_capacity(doc_versions.len());
        for (doc_path, version) in &doc_versions {
            let Some((file, _)) = self.parsed_file(doc_path).await else {
                continue;
            };
            open_files.push((doc_path.clone(), *version, file));
        }
        // Canonical project order (#490): disk entries sorted by path,
        // then open documents sorted by path. Project shadowing follows
        // insertion order (the last file wins), so assembling from
        // unsorted HashMaps made the winning definition depend on the
        // process's hash seed, and close/reopen moved the reopened file
        // to the end and flipped the winner. Sorting gives the LSP the
        // CLI's contract (discovery output is sorted by path); open
        // documents are layered last so the editor's buffer — the
        // authoritative content for its path — shadows same-named
        // definitions from indexed disk files. Disk files never shadow
        // open documents; files in disabled folders are dropped by the
        // same eligibility rule as open ones. Every open path shadows its
        // disk twin — not just the eligible ones — so a buffer that grew
        // past `max-file-bytes` cannot keep contributing its path through
        // the stale small on-disk twin the index still holds (#488): the
        // open buffer owns its path, eligible or not.
        let mut project_files: Vec<(String, i32, Arc<SourceFile>)> = {
            let state = self.state.lock().await;
            let open_paths: std::collections::HashSet<&str> =
                state.docs.keys().map(String::as_str).collect();
            let mut disk_entries: Vec<(String, i32, Arc<SourceFile>)> = state
                .disk_files
                .iter()
                .filter(|(p, _)| state.eligibility_for_path(p))
                .filter(|(p, _)| !open_paths.contains(p.as_str()))
                .map(|(p, file)| (p.clone(), 0, Arc::clone(file)))
                .collect();
            disk_entries.sort_by(|a, b| a.0.cmp(&b.0));
            disk_entries
        };
        open_files.sort_by(|a, b| a.0.cmp(&b.0));
        project_files.extend(open_files);

        // Check each folder partition independently through its own
        // ProjectCache, stubs, and workspace context. The root-level
        // filter, confidence, exclude, baseline, and root state rides
        // along for the files no folder owns.
        let (
            folder_contexts,
            root_project,
            user_stubs,
            root_filter,
            root_min_confidence,
            root_excludes,
            root_baseline,
            root,
            root_config_dir,
        ) = {
            let state = self.state.lock().await;
            (
                state.folder_contexts.clone(),
                Arc::clone(&state.project),
                Arc::clone(&state.user_stubs),
                state.root_filter.clone(),
                state.root_min_confidence,
                state.root_excludes.clone(),
                state.root_baseline.clone(),
                state.root.clone(),
                state.root_config_dir.clone(),
            )
        };

        // Partition project_files by folder root: each file goes to the
        // first folder context whose root contains it (the same ownership
        // rule as `folder_context_for_path`); files no folder owns go to
        // the root project. The contexts are already clones, so every
        // partition carries its owning context instead of a map key
        // nobody reads.
        let mut partitions: Vec<FolderPartition> = Vec::new();
        let mut root_files: Vec<(String, i32, Arc<SourceFile>)> = Vec::new();
        for (fp, ver, file) in project_files {
            if let Some(ctx) = folder_contexts
                .iter()
                .find(|c| std::path::Path::new(&fp).starts_with(&c.root))
            {
                let partition = partitions
                    .iter_mut()
                    .find(|(owned, _)| owned.as_ref().is_some_and(|c| c.root == ctx.root));
                match partition {
                    Some((_, files)) => files.push((fp, ver, file)),
                    None => partitions.push((Some(ctx.clone()), vec![(fp, ver, file)])),
                }
            } else {
                root_files.push((fp, ver, file));
            }
        }
        if !root_files.is_empty() {
            partitions.push((None, root_files));
        }

        let mut all_results: Vec<(Option<FolderAnalysisContext>, ProjectCheckResult)> = Vec::new();
        for (ctx, files) in partitions {
            let (stubs, workspace_context, project_handle) = match &ctx {
                Some(ctx) => (
                    Arc::clone(&ctx.stubs),
                    ctx.workspace_context.clone(),
                    Arc::clone(&ctx.project_cache),
                ),
                None => (Arc::clone(&user_stubs), None, Arc::clone(&root_project)),
            };
            let mut project = project_handle.lock().await;
            let result = project.check_with_workspace(files, stubs, workspace_context.as_ref());
            all_results.push((ctx, result));
        }

        // An edit that arrived while parsing/checking invalidates this whole
        // project result because every open document is republished below.
        {
            let state = self.state.lock().await;
            if state.diag_generation != generation {
                return;
            }
        }

        let diagnostic_versions: HashMap<_, _> = doc_versions.into_iter().collect();

        // Publish per-file diagnostics through the folder's
        // filter/confidence/exclude/baseline state. The partition carried
        // the owning context through the check, so publication uses the
        // same snapshot the check ran under; files no folder owns use the
        // snapshotted root-level values. `published` records what this
        // pass sent (and whether it was non-empty) for the tracking
        // reconciliation below.
        let mut published: Vec<(String, bool)> = Vec::new();
        for (ctx, result) in all_results {
            let ProjectCheckResult {
                diagnostics: per_file,
                files: checked_files,
            } = result;
            let (filter, min_confidence, excludes, baseline, config_anchor) = match ctx.as_ref() {
                Some(ctx) => (
                    ctx.filter.clone(),
                    ctx.min_confidence,
                    ctx.excludes.clone(),
                    ctx.baseline.clone(),
                    // Config-relative exclude patterns and baseline keys
                    // anchor at the originating `ry.toml`'s directory —
                    // the same `repo_root` `ry check` derives from its
                    // discovered config (#493) — not at the folder root.
                    ctx.config_root.clone().or_else(|| Some(ctx.root.clone())),
                ),
                None => (
                    root_filter.clone(),
                    root_min_confidence,
                    root_excludes.clone(),
                    root_baseline.clone(),
                    root_config_dir.clone().or(root.clone()),
                ),
            };
            for (diagnostic_path, diagnostics) in per_file {
                if !excludes.is_empty() {
                    let rel =
                        ry_config::diagnostic_path(&diagnostic_path, config_anchor.as_deref());
                    if excludes.matches(&rel) {
                        continue;
                    }
                }

                // Post-processing runs through the shared pipeline
                // (`ry_checker::post_process`) so the editor sees exactly
                // what `ry check` reports: inline suppression comments,
                // then the severity filter, then path-based confidence
                // demotion, then baseline subtraction, then the
                // min-confidence threshold. Subtracting the baseline
                // before the suppression filter let a suppressed
                // occurrence consume the count for its unsuppressed twin
                // (#491); skipping the demotion stage kept support-tree
                // findings above the threshold in the editor after
                // `ry check` had dropped them (#492).
                let checked_file = checked_files.get(&diagnostic_path);
                let source_text = checked_file.map(|file| file.source.as_str());
                let comments: &[ry_core::ast::Comment] =
                    checked_file.map_or(&[], |file| file.comments.as_slice());
                let post = ry_checker::PostProcess {
                    filter: &filter,
                    baseline: baseline.as_ref(),
                    min_confidence: min_confidence.unwrap_or(ry_checker::Confidence::Low),
                    repo_root: config_anchor.as_deref(),
                };
                let mut diagnostics =
                    post.pre_demotion(diagnostics, comments, source_text.unwrap_or(""));
                post.demote_non_source_paths(&mut diagnostics);
                post.post_demotion(&mut diagnostics);
                let diagnostics: Vec<LspDiagnostic> = diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        let mut diagnostic = match source_text {
                            Some(text) => diagnostic_to_lsp_with_source(&diagnostic, text),
                            None => diagnostic_to_lsp(diagnostic),
                        };
                        if supports_diagnostic_data
                            && let Some(version) = diagnostic_versions.get(&diagnostic_path)
                        {
                            diagnostic.data =
                                Some(diagnostic_origin(&diagnostic_path, *version, generation));
                        }
                        diagnostic
                    })
                    .collect();
                let diagnostic_uri = path_to_uri(&diagnostic_path);
                let non_empty = !diagnostics.is_empty();
                self.client
                    .publish_diagnostics(diagnostic_uri, diagnostics, None)
                    .await;
                published.push((diagnostic_path, non_empty));
            }
        }

        // Reconcile the tracked publication set against this pass: a URI
        // whose last publication carried diagnostics but which receives
        // none now — its folder was disabled, discovery excludes it, or a
        // rescan dropped the closed file from the index — keeps its old
        // squiggles in the editor forever unless it is explicitly
        // cleared (#489).
        {
            let mut state = self.state.lock().await;
            for (path, non_empty) in published {
                if non_empty {
                    state.published_paths.insert(path);
                } else {
                    state.published_paths.remove(&path);
                }
            }
        }
        self.clear_dropped_diagnostics().await;
    }

    /// Publish empty diagnostics for every tracked URI that left the
    /// analysis set since its last publication: the path is no longer
    /// eligible (folder disabled, file excluded by a settings or config
    /// change), or it is neither open nor in the disk index (a closed
    /// file deleted or dropped by a rescan). Called at the end of every
    /// publish pass, and after a rescan no open document will follow up
    /// on. Deciding against current state — not against one pass's
    /// snapshot — also self-heals a slower, older pass that publishes
    /// after a newer one already cleared a URI (#489).
    async fn clear_dropped_diagnostics(&self) {
        let dropped: Vec<Url> = {
            let mut state = self.state.lock().await;
            let dropped: Vec<String> = state
                .published_paths
                .iter()
                .filter(|path| {
                    !state.eligibility_for_path(path.as_str())
                        || (!state.docs.contains_key(path.as_str())
                            && !state.disk_files.contains_key(path.as_str()))
                })
                .cloned()
                .collect();
            for path in &dropped {
                state.published_paths.remove(path);
            }
            dropped.iter().map(|p| path_to_uri(p)).collect()
        };
        for uri in dropped {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }

    /// Discover and parse all `.R`/`.r` files under the workspace root(s)
    /// in a background task and store the results in `state.disk_files`.
    /// This function never publishes diagnostics itself: callers await it
    /// and then republish (e.g. `did_change_watched_files`), which is what
    /// makes cross-file calls into unopened files resolve on the next
    /// check. Returns false when a newer scan or folder change supersedes it;
    /// the superseding caller then owns the republish.
    async fn spawn_background_index(&self) -> bool {
        let (roots_with_config, index_gen) = {
            let mut state = self.state.lock().await;
            state.index_generation = state.index_generation.wrapping_add(1);
            let idx_gen = state.index_generation;
            let roots = if !state.folder_contexts.is_empty() {
                state
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
                    .collect()
            } else if let Some(root) = &state.root {
                vec![(
                    root.clone(),
                    state.root_config_dir.clone(),
                    state.file_config.clone(),
                    Arc::clone(&state.user_stubs),
                )]
            } else {
                Vec::new()
            };
            (roots, idx_gen)
        };
        if roots_with_config.is_empty() {
            let mut state = self.state.lock().await;
            if state.index_generation == index_gen {
                state.initial_index_pending = false;
                return true;
            }
            return false;
        }

        #[cfg(feature = "test-util")]
        crate::test_seam::maybe_pause_initial_index().await;
        let indexed = tokio::task::spawn_blocking(move || {
            let mut all_disk_files: HashMap<String, Arc<SourceFile>> = HashMap::new();
            let mut contexts = Vec::new();
            let mut all_truncated: Vec<(PathBuf, ry_workspace::TruncationReport)> = Vec::new();
            for (root, config_root, config, stubs) in &roots_with_config {
                let outcome = crate::index::index_workspace(root, config_root.as_deref(), config);
                if outcome.truncated.iter().any(|t| t.any_hit()) {
                    for report in &outcome.truncated {
                        all_truncated.push((root.clone(), report.clone()));
                    }
                }
                let files: Vec<&SourceFile> = outcome.files.values().map(AsRef::as_ref).collect();
                match ry_workspace::resolve_workspace_context(
                    root,
                    config,
                    ry_workspace::ResolutionEnvironment {
                        files,
                        user_stubs: stubs,
                    },
                ) {
                    Ok(context) => contexts.push((root.clone(), context)),
                    Err(error) => tracing::warn!(%error, "workspace resolution degraded"),
                }
                all_disk_files.extend(outcome.files);
            }
            (all_disk_files, contexts, all_truncated)
        })
        .await;

        match indexed {
            Ok((disk_files, contexts, truncated)) => {
                tracing::info!(
                    files = disk_files.len(),
                    "background workspace index complete"
                );
                // A cap hit is never silent: structured events below plus
                // one user-visible warning per scan generation.
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
                let cap_hit = !truncated.is_empty();
                let mut state = self.state.lock().await;
                if state.index_generation != index_gen {
                    tracing::debug!(
                        gen = index_gen,
                        current = state.index_generation,
                        "discarding stale background index results"
                    );
                    return false;
                }
                state.disk_files = disk_files;
                for ctx in &mut state.folder_contexts {
                    if let Some((_, wc)) = contexts.iter().find(|(root, _)| root == &ctx.root) {
                        ctx.workspace_context = Some(wc.clone());
                    }
                }
                state.initial_index_pending = false;
                drop(state);
                for (_, context) in &contexts {
                    for (path, reason) in &context.degraded_scopes {
                        self.client.log_message(
                            tower_lsp::lsp_types::MessageType::WARNING,
                            format!("ry: {}: {reason}; using a file-stem binding. Raise max-serialized-bytes in ry.toml to enumerate it.", path.display()),
                        ).await;
                    }
                }
                if cap_hit {
                    let _ = self
                        .client
                        .log_message(
                            tower_lsp::lsp_types::MessageType::WARNING,
                            "ry: discovery cap reached; some R files were not indexed. See server logs for details (index.max-files / index.max-file-bytes / index.max-depth).",
                        )
                        .await;
                }
                true
            }
            Err(error) => {
                tracing::warn!(%error, "background workspace index failed");
                let mut state = self.state.lock().await;
                if state.index_generation == index_gen {
                    state.initial_index_pending = false;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Schedule a diagnostics republish for every open document. Used
    /// after a global state change (settings, watched files) so the new
    /// state takes effect immediately without waiting for an edit. With
    /// no open document there is nothing to drive a debounced pass, so
    /// the dropped-URI reconciliation runs directly instead — closed
    /// disk files the rescan just dropped must not keep their last
    /// publications (#489).
    async fn republish_all_open_documents(&self) {
        let open_uris: Vec<Url> = {
            let state = self.state.lock().await;
            state.docs.keys().map(|p| path_to_uri(p)).collect()
        };
        if open_uris.is_empty() {
            self.clear_dropped_diagnostics().await;
            return;
        }
        for uri in open_uris {
            self.schedule_diagnostics(uri).await;
        }
    }

    /// Debounce diagnostics: bump the workspace generation counter and
    /// spawn a task that sleeps ~180ms, then publishes only if its
    /// generation is still the latest. A newer edit during the sleep
    /// window bumps the counter and the stale task aborts, so a burst of
    /// keystrokes triggers a single check rather than one per keystroke.
    /// Diagnostics are project-wide, so one workspace generation
    /// coalesces edits across all open documents: every scheduled URI
    /// joins `pending_diag_paths`, and the one surviving task drains and
    /// publishes the whole set. Scheduling N URIs — exactly what
    /// `republish_all_open_documents` does — used to leave only the last
    /// task alive, aborting the other N-1 as stale so every URI but the
    /// last scheduled kept its previous diagnostics (#489).
    async fn schedule_diagnostics(&self, uri: Url) {
        let generation = {
            let mut state = self.state.lock().await;
            state.pending_diag_paths.insert(uri_to_path(&uri));
            state.diag_generation = state.diag_generation.wrapping_add(1);
            state.diag_generation
        };
        let backend = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            // Drain under the lock so the take is atomic with the
            // staleness check: only the task holding the latest
            // generation publishes, and it takes every URI scheduled in
            // the burst (a newer schedule would have made this task
            // stale and left the set for its own task).
            let requested = {
                let mut state = backend.state.lock().await;
                (state.diag_generation == generation)
                    .then(|| std::mem::take(&mut state.pending_diag_paths))
            };
            if let Some(requested) = requested {
                backend.publish_diagnostics(requested, generation).await;
                #[cfg(feature = "test-util")]
                crate::test_seam::note_initial_diagnostic_cycle();
            }
        });
    }
}

/// Load the root `ry.toml` and the user stubs it declares. A missing or
/// broken root config degrades to defaults (with a warning on breakage)
/// and empty stubs — never a fatal error. Returns the config paired with
/// the directory its `ry.toml` was loaded from (`None` when defaults are
/// in use), anchoring the root-level fallback's config-relative paths
/// like the CLI anchors them at the config directory (#493). Disk I/O
/// happens here; run it off the async runtime.
fn load_root_config_and_stubs(
    root: Option<&std::path::Path>,
) -> (
    Option<PathBuf>,
    ry_config::Config,
    Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
) {
    let (config_dir, config) = match root {
        // `load_from_dir` looks only inside `root` itself, so a found
        // config's directory IS the root.
        Some(root) => match ry_config::Config::load_from_dir(root) {
            Ok(Some(config)) => (Some(root.to_path_buf()), config),
            Ok(None) => (None, ry_config::Config::default()),
            Err(error) => {
                tracing::warn!(
                    root = %root.display(),
                    %error,
                    "failed to load root ry.toml; using default config"
                );
                (None, ry_config::Config::default())
            }
        },
        None => (None, ry_config::Config::default()),
    };
    let stubs = load_stubs_from_config(&config).unwrap_or_default();
    (config_dir, config, stubs)
}

/// Marker for a stub load where the config declares typeshed directories
/// and every one of them failed. Each per-directory failure is logged
/// where it happens; the marker exists only so reload callers can tell a
/// genuine reload failure apart from an intentionally empty stub map.
#[derive(Debug)]
struct AllStubDirsFailed;

/// Load the stubs a loaded config's typeshed directories declare;
/// per-folder use keeps two roots defining the same package differently
/// isolated. `Ok` is the intended new state — empty when the config
/// declares no directories, or when the loaded ones ship no (valid)
/// stubs. `Err` means directories were configured and every one failed
/// to load, letting reload callers retain their previous stub snapshot.
fn load_stubs_from_config(
    config: &ry_config::Config,
) -> Result<Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>, AllStubDirsFailed> {
    let mut merged = std::collections::BTreeMap::new();
    let mut loaded_any = false;
    for dir in &config.typeshed {
        match ry_typeshed::load_stub_dir_with_warnings(dir) {
            Ok((stubs, warnings)) => {
                loaded_any = true;
                merged.extend(stubs);
                for warning in warnings {
                    tracing::warn!(%warning, "skipping malformed user stub");
                }
            }
            Err(error) => tracing::warn!(%error, "failed to load user stub directory"),
        }
    }
    if loaded_any || config.typeshed.is_empty() {
        Ok(Arc::new(merged))
    } else {
        Err(AllStubDirsFailed)
    }
}

fn custom_config_paths(state: &State) -> Vec<PathBuf> {
    let mut paths: Vec<_> = state
        .folder_contexts
        .iter()
        .filter_map(|ctx| {
            ctx.folder_settings.configuration.as_ref().and_then(|path| {
                // Parsing normalizes dot segments as client event URIs do;
                // from_file_path alone preserves them.
                let uri = Url::from_file_path(ctx.root.join(path)).ok()?;
                Url::parse(uri.as_str()).ok()?.to_file_path().ok()
            })
        })
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

// LSP does not define literal escaping. Use VS Code/minimatch's bracket
// convention; support for literal brackets varies across client engines.
fn escape_watch_path(path: &str) -> String {
    path.chars()
        .map(|ch| match ch {
            '*' | '?' | '[' | ']' | '{' | '}' => format!("[{ch}]"),
            _ => ch.to_string(),
        })
        .collect()
}

/// Lexically normalize a config directory's dot segments
/// (`config/../config` → `config`) without touching the filesystem, the
/// same normalization client event URIs receive. The config anchor must
/// prefix-match the clean absolute paths diagnostics and discovery
/// entries carry, so a `configuration` setting containing `..` segments
/// cannot keep them raw (#493).
fn normalize_config_dir(dir: &Path) -> PathBuf {
    Url::from_file_path(dir)
        .ok()
        .and_then(|uri| Url::parse(uri.as_str()).ok())
        .and_then(|uri| uri.to_file_path().ok())
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Load a folder's explicit configuration or discover its nearest `ry.toml`.
/// Missing discovered configuration uses defaults; read and parse failures
/// reach the caller so reloads can retain the last valid configuration.
/// Returns the effective config paired with the directory of the `ry.toml`
/// it was loaded from (`None` when defaults are in use) — the same origin
/// the CLI keeps as its config root — so config-relative `exclude`
/// patterns, `include-build-ignored`, and baseline keys anchor at the
/// config's own directory instead of the workspace folder (#493).
/// Callers must keep this disk I/O outside the state lock.
fn discover_folder_config(
    folder_settings: &FolderSettings,
    folder_root: &std::path::Path,
) -> std::result::Result<(Option<PathBuf>, ry_config::Config), ry_config::ConfigError> {
    if let Some(config_path) = &folder_settings.configuration {
        // The anchor `load_file` itself rebases `typeshed`/`baseline`
        // against; both stay anchored at one origin, so nothing is
        // double-rebased.
        let path = folder_root.join(config_path);
        let config = ry_config::Config::load_file(&path)?;
        return Ok((path.parent().map(normalize_config_dir), config));
    }
    match ry_config::Config::discover(folder_root)? {
        Some((path, config)) => Ok((path.parent().map(normalize_config_dir), config)),
        None => Ok((None, ry_config::Config::default())),
    }
}

/// Resolve the baseline path from editor settings / `ry.toml` and load it
/// from disk. `Ok(None)` means no baseline is configured; `Err` signals a
/// configured-but-unloadable baseline so reload callers can retain the
/// last valid value rather than silently clearing it. Disk I/O happens
/// here — callers MUST run this outside the state lock.
fn load_folder_baseline(
    settings: &FolderSettings,
    config: &ry_config::Config,
    folder_root: Option<&std::path::Path>,
) -> std::result::Result<Option<ry_config::Baseline>, String> {
    let baseline_path = match settings
        .baseline
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| config.baseline.clone())
    {
        Some(p) => p,
        None => return Ok(None),
    };
    let resolved = if baseline_path.is_relative() {
        match folder_root.map(|r| r.join(&baseline_path)) {
            Some(r) => r,
            None => return Ok(None),
        }
    } else {
        baseline_path
    };
    // Baseline-read accounting for the `test-util` counter above; see
    // `baseline_disk_reads` (#170).
    #[cfg(feature = "test-util")]
    BASELINE_DISK_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ry_config::load_baseline(&resolved)
        .map(Some)
        .map_err(|e| format!("{e}"))
}

/// Build per-folder analysis contexts for every workspace root: effective
/// `ry.toml` config (directory discovery or the editor `configuration`
/// override), editor [`FolderSettings`], local typesheds, and the cached
/// baseline. When `workspace_folders` is empty, `root_uri` becomes the
/// single folder.
pub(super) fn build_folder_contexts(
    root: Option<&std::path::Path>,
    workspace_folders: &[(usize, PathBuf)],
    server_settings: &ServerSettings,
) -> Vec<FolderAnalysisContext> {
    let folders: Vec<(usize, PathBuf)> = if !workspace_folders.is_empty() {
        workspace_folders.to_vec()
    } else if let Some(root) = root {
        vec![(0, root.to_path_buf())]
    } else {
        return Vec::new();
    };

    let mut contexts = Vec::with_capacity(folders.len());
    for (settings_idx, folder_root) in &folders {
        // Per-folder editor settings: index-correlated entry or global fallback.
        let folder_settings = server_settings
            .settings
            .get(*settings_idx)
            .cloned()
            .unwrap_or_else(|| server_settings.global_settings.clone());

        // Discover config (defaulting on failure); the baseline loads once
        // here so the publish path never touches disk.
        let (config_root, config) = discover_folder_config(&folder_settings, folder_root)
            .unwrap_or_else(|error| {
                tracing::warn!(
                    root = %folder_root.display(),
                    %error,
                    "failed to load folder config; using default config"
                );
                (None, ry_config::Config::default())
            });
        let baseline = match load_folder_baseline(&folder_settings, &config, Some(folder_root)) {
            Ok(opt) => opt,
            Err(error) => {
                tracing::warn!(
                    root = %folder_root.display(),
                    %error,
                    "failed to load baseline; no baseline cached for this folder"
                );
                None
            }
        };

        // A fresh build has no previous snapshot to retain, so a failed
        // directory load degrades to empty stubs.
        let stubs = load_stubs_from_config(&config).unwrap_or_default();

        let (filter, min_confidence, excludes) = compute_folder_filter(&config, &folder_settings);
        contexts.push(FolderAnalysisContext {
            root: folder_root.clone(),
            config_root,
            config,
            folder_settings,
            stubs,
            workspace_context: None,
            baseline,
            filter,
            min_confidence,
            excludes,
            project_cache: Arc::new(Mutex::new(ProjectCache::default())),
        });
    }

    // Longest-prefix ownership: sort by root path length descending.
    contexts.sort_by_key(|ctx| std::cmp::Reverse(ctx.root.as_os_str().len()));
    contexts
}

/// Rebuild a single folder's analysis context from disk.
///
/// Each field is reloaded independently. On a sub-failure (config parse,
/// baseline parse, or a stub reload where EVERY configured typeshed
/// directory failed) the last valid value for that field is retained and
/// the failure is logged — a corrupt reload never silently clears the
/// baseline, and a fully failed stub reload never silently drops the
/// stub map. One exception: a failed baseline reload after the config's
/// directory changed clears the baseline instead of retaining it,
/// because the retained keys are relative to the old anchor (#493). A
/// partial stub failure keeps only the directories that
/// loaded, replacing the map with what a fresh server would produce.
/// Deliberate removals (a setting deleted or set to an empty list) are
/// not failures and always take effect. `folder_settings`,
/// `workspace_context`, and `project_cache` are not config-file-derived
/// (they come from editor push / the background indexer / incremental
/// checks respectively) and are carried over unchanged. Disk I/O
/// happens here; callers MUST run this outside the state lock.
pub(super) fn rebuild_folder_context(old: &FolderAnalysisContext) -> FolderAnalysisContext {
    let (config_root, config) = match discover_folder_config(&old.folder_settings, &old.root) {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(
                root = %old.root.display(),
                %error,
                "failed to reload folder config; retaining previous config"
            );
            (old.config_root.clone(), old.config.clone())
        }
    };
    // Reload stubs from the (possibly retained) config. An empty result
    // is the intended new state — the config declares no typeshed
    // directories, or the loaded ones ship no stubs — so removing the
    // last directory clears the map and a warm session converges with a
    // fresh server. Only a reload where every configured directory
    // failed retains the previous stubs.
    let stubs = match load_stubs_from_config(&config) {
        Ok(new_stubs) => new_stubs,
        Err(AllStubDirsFailed) => {
            tracing::warn!(
                root = %old.root.display(),
                "every configured typeshed directory failed to reload; retaining previous stubs"
            );
            old.stubs.clone()
        }
    };
    let baseline = match load_folder_baseline(&old.folder_settings, &config, Some(&old.root)) {
        Ok(opt) => opt,
        Err(error) => {
            // A retained baseline's keys stay relative to the anchor it
            // was loaded under, so it may only be retained while the
            // config origin is unchanged; pairing old keys with a new
            // anchor would match the wrong files (#493).
            if config_root == old.config_root {
                tracing::warn!(
                    root = %old.root.display(),
                    %error,
                    "failed to reload baseline; retaining last valid baseline"
                );
                old.baseline.clone()
            } else {
                tracing::warn!(
                    root = %old.root.display(),
                    %error,
                    "failed to reload baseline after the config moved; clearing the stale baseline"
                );
                None
            }
        }
    };
    let (filter, min_confidence, excludes) = compute_folder_filter(&config, &old.folder_settings);
    FolderAnalysisContext {
        root: old.root.clone(),
        config_root,
        config,
        folder_settings: old.folder_settings.clone(),
        stubs,
        workspace_context: old.workspace_context.clone(),
        baseline,
        filter,
        min_confidence,
        excludes,
        project_cache: Arc::clone(&old.project_cache),
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

/// Convert an LSP range (0-based line/character, UTF-16 code units) in
/// `old_text` to a byte-offset span for splicing.
fn range_byte_span(old_text: &str, range: Range) -> Option<(usize, usize)> {
    let start_byte = position_to_byte_offset(old_text, range.start.line, range.start.character)?;
    let end_byte = position_to_byte_offset(old_text, range.end.line, range.end.character)?;
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
    let new_end_position = byte_offset_to_point_relative(start_position, new_text);

    ry_core::InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position,
        old_end_position,
        new_end_position,
    }
}

/// Compute the new end Point after inserting `new_text` at the position
/// where `start_position` sits.
fn byte_offset_to_point_relative(start_position: ry_core::Point, new_text: &str) -> ry_core::Point {
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
