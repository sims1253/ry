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
use crate::hints::collect_inlay_hints;
use crate::settings::{FolderSettings, ServerSettings};
use crate::util::position_to_byte_offset_pos;
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

/// Counts baseline file reads performed by `load_folder_baseline`
/// (the only baseline disk-read site in the LSP). Reads of publish/inlay-hint
/// state use the cached [`FolderAnalysisContext`] and never touch
/// this counter. Exposed via [`baseline_disk_reads`] so integration tests can
/// assert hot-path I/O is absent rather than infer it from timing.
static BASELINE_DISK_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// number of baseline file reads since process start. A
/// publish/inlay-hint that does not change this value performs
/// zero baseline disk I/O.
pub fn baseline_disk_reads() -> usize {
    BASELINE_DISK_READS.load(std::sync::atomic::Ordering::Relaxed)
}

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
    /// cached parse lets every handler avoid re-parsing on each request.
    /// `SourceFile` is `Send`; `RParser` is NOT, so the
    /// parser is constructed per request and only the result is cached.
    parsed: HashMap<String, (i32, Arc<SourceFile>)>,
    /// path -> (version, top-level Scope from `check_with_scope`).
    /// Reused by inlay_hint so it doesn't re-run the
    /// single-file check on every request. Invalidated
    /// by `update_doc` alongside the parse cache.
    scopes: HashMap<String, (i32, ry_checker::Scope)>,
    /// Workspace-wide debounce counter. `schedule_diagnostics` bumps this and
    /// spawns a task that sleeps, then only publishes if its generation
    /// is still the latest. A newer edit during the
    /// sleep window wins and the stale task aborts.
    diag_generation: u64,
    /// Index generation stamp. Bumped every time
    /// `spawn_background_index` starts so that results from a prior
    /// folder set are discarded. The background task captures the
    /// generation at dispatch and checks it before writing.
    index_generation: u64,
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
    /// The full `ry-config::Config` loaded from `ry.toml` at the
    /// workspace root. Stored so the severity filter and baseline can
    /// be applied in `publish_diagnostics` without re-reading the file.
    file_config: ry_config::Config,
    /// Root-level baseline cached at initialize so the
    /// fallback publish path (documents outside every folder root) performs
    /// no disk access. Reloaded alongside the root config on watch events.
    root_baseline: Option<ry_config::Baseline>,
    /// Root-level filter, confidence threshold, and excludes, precomputed
    /// from `file_config` and the root `folder_settings` at initialize and
    /// refreshed on reload — the same treatment `root_baseline` gets.
    ///
    /// A document outside every folder root must be filtered by root-level
    /// config. Borrowing `folder_contexts.first()` instead applied an
    /// unrelated root's severity filter and exclude globs (that vec is sorted
    /// by root-path length descending, so `.first()` is the *most specific*
    /// unrelated root, whose excludes could drop the diagnostics entirely).
    root_filter: ry_checker::SeverityFilter,
    root_min_confidence: Option<ry_checker::Confidence>,
    root_excludes: ry_config::Excludes,
    /// Editor-supplied per-folder settings, received via
    /// `initializationOptions`, `workspace/configuration`, or
    /// `didChangeConfiguration`.
    folder_settings: FolderSettings,
    /// The full server settings envelope received at
    /// initialize. Retained so dynamically added workspace folders can
    /// be built through the same `build_folder_contexts` path as initial
    /// folders.
    server_settings: ServerSettings,
    /// Whether the client supports the `workspace/configuration`
    /// pull request. When true, `didChangeConfiguration` re-pulls
    /// instead of parsing the notification payload.
    supports_workspace_configuration: bool,
    /// Whether the client supports dynamic registration of
    /// `workspace/didChangeWatchedFiles`.
    supports_did_change_watched_files: bool,

    // --- multi-root workspace folders ---
    /// Per-root analysis context holding effective config, editor settings,
    /// local typesheds, and the workspace context. Ordered by root
    /// path length descending for longest-prefix ownership.
    folder_contexts: Vec<FolderAnalysisContext>,
    /// Per-folder project caches for isolated checking. Each workspace
    /// folder gets its own `ProjectCache` so two roots defining the same
    /// package differently never collide. Keyed by root path string.
    folder_projects: HashMap<String, Arc<Mutex<ProjectCache>>>,
    /// On-disk `.R`/`.r` files discovered by the background indexer
    /// Keyed by absolute path. Open documents shadow
    /// these — when a path exists in both `docs` and `disk_files`,
    /// the open document's content is authoritative.
    disk_files: HashMap<String, Arc<SourceFile>>,
    /// Version-stamped tree-sitter trees for incremental
    /// parsing. Each entry stores the document version
    /// (generation) the tree was produced from. A parse result may
    /// replace the cache only if its version still matches the current
    /// document version; a cache read returns the tree only under the
    /// same invariant, so no cached tree can ever be served for a
    /// different document generation.
    trees: HashMap<String, (i32, ry_core::Tree)>,
    /// per-server counter for filter/glob construction events.
    /// Incremented each time a filter or exclude set is compiled from config.
    /// Scoped to this server (not process-global) so parallel integration
    /// tests each observe only their own compilations, never a sibling's.
    pub(super) filter_compile_count: Arc<std::sync::atomic::AtomicU64>,
    /// compile-count delta observed during this server's most
    /// recent `publish_diagnostics` cycle. Tests assert it is zero
    /// (precomputed values are borrowed, never recompiled mid-publish).
    pub(super) compile_during_last_publish: Arc<std::sync::atomic::AtomicU64>,
}

/// One per-folder analysis context.
///
/// Factored from longest-prefix ownership so every analysis channel
/// resolves config, editor settings, local typesheds, and package
/// metadata through the owning folder rather than a single server-wide
/// value. Two roots may define the same package differently without
/// collision.
#[derive(Clone, Default)]
pub(super) struct FolderAnalysisContext {
    /// The workspace folder root directory.
    pub root: PathBuf,
    /// Effective `ry.toml` config: loaded from directory discovery or
    /// the editor `configuration` override resolved relative to `root`.
    pub config: ry_config::Config,
    /// Editor-supplied per-folder settings.
    pub folder_settings: FolderSettings,
    /// Local typeshed stubs loaded from this folder's `ry.toml`.
    pub stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Workspace resolution context for package metadata.
    pub workspace_context: Option<ry_workspace::WorkspaceContext>,
    /// The baseline loaded from `ry.toml`/editor settings,
    /// cached during context construction so the publish path performs no
    /// disk access. Reloaded outside the state lock on watch events; a
    /// failed reload retains the last valid value (see
    /// `rebuild_folder_context`).
    pub baseline: Option<ry_config::Baseline>,
    /// Precomputed severity filter for this folder,
    /// compiled once during context construction instead of per-file
    /// in the publish loop.
    pub filter: ry_checker::SeverityFilter,
    /// Precomputed minimum confidence threshold.
    pub min_confidence: Option<ry_checker::Confidence>,
    /// Precompiled exclude glob patterns.
    pub excludes: ry_config::Excludes,
}

/// Compute the precomputed filter, min_confidence, and
/// excludes for a folder from its config and settings. This replaces
/// the per-file reconstruction that previously happened inside
/// `publish_diagnostics`. The folder config is both the exclude source
/// and the severity fallback.
///
/// `filter_count` is this server's [`State::filter_compile_count`]; the
/// increment is threaded in explicitly (rather than touching a process
/// global) so parallel test servers never observe each other's compiles.
fn compute_folder_filter(
    config: &ry_config::Config,
    folder_settings: &FolderSettings,
    filter_count: &std::sync::atomic::AtomicU64,
) -> (
    ry_checker::SeverityFilter,
    Option<ry_checker::Confidence>,
    ry_config::Excludes,
) {
    filter_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

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
/// folder context and the root-level fallback from their (already
/// installed) `folder_settings`.
///
/// Factored out so the initial pull in `initialized` and the refresh pull
/// in `did_change_configuration` share one cache-refresh path and cannot
/// drift; the push-based `did_change_configuration` path uses it too for
/// the same reason. Each folder mirrors `rebuild_folder_context` (the
/// folder config is both the exclude source and the severity fallback);
/// the root mirrors `initialize`. This stays OUT of `publish_diagnostics`:
/// the publish-cycle contract asserts zero filter compilations *during a
/// publish cycle*; recomputing on a configuration change is the same
/// off-publish treatment the push-based path already uses.
fn refresh_cached_folder_filters(state: &mut State) {
    let filter_count = Arc::clone(&state.filter_compile_count);
    for ctx in &mut state.folder_contexts {
        let (filter, min_confidence, excludes) =
            compute_folder_filter(&ctx.config, &ctx.folder_settings, &filter_count);
        ctx.filter = filter;
        ctx.min_confidence = min_confidence;
        ctx.excludes = excludes;
    }
    let (root_filter, root_min_confidence, root_excludes) =
        compute_folder_filter(&state.file_config, &state.folder_settings, &filter_count);
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

    /// Drop the cached parse and scope for `path`, mirroring the
    /// cache-invalidation half of `Backend::update_doc`. Test-only;
    /// lets the cache acceptance test simulate a `did_change` on a bare
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

    // --- effective config computation ---

    /// Test helper: compute the effective `SeverityFilter` from editor
    /// settings merged over the root `ry.toml`, through the same
    /// [`compute_folder_filter`] the production paths use (excludes and
    /// min_confidence are discarded here; the tests assert the filter).
    #[cfg(test)]
    pub(super) fn effective_filter(&self) -> ry_checker::SeverityFilter {
        compute_folder_filter(
            &self.file_config,
            &self.folder_settings,
            &self.filter_compile_count,
        )
        .0
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
    /// `doc_path`. A folder whose editor settings set `enable: false`
    /// is skipped entirely; otherwise eligibility follows the owning
    /// folder's discovery rules.
    fn eligibility_for_path(&self, doc_path: &str) -> bool {
        let path = std::path::Path::new(doc_path);
        if let Some(ctx) = self.folder_context_for_path(doc_path) {
            if ctx.folder_settings.enable == Some(false) {
                return false;
            }
            return ry_workspace::is_file_eligible(path, &ctx.root, &ctx.config);
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
                ry_workspace::is_file_eligible(path, root, &self.file_config)
            }
            _ => true,
        }
    }

    /// Return the cached effective baseline for a document path.
    ///
    /// This is a pure cache read — it performs no disk access.
    /// The baseline is loaded into each [`FolderAnalysisContext`] (and the
    /// root-level fallback) during [`initialize`](Backend::initialize) and
    /// reloaded *outside* the state lock on watch events by
    /// [`rebuild_folder_contexts`]. Editor setting takes precedence over
    /// `ry.toml`; both were resolved relative to the owning folder root at
    /// load time.
    pub(super) fn effective_baseline_for_path(
        &self,
        doc_path: &str,
    ) -> Option<ry_config::Baseline> {
        if let Some(ctx) = self.folder_context_for_path(doc_path) {
            return ctx.baseline.clone();
        }
        self.root_baseline.clone()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let root = params.root_uri.and_then(|uri| uri.to_file_path().ok());

        // Read initializationOptions if present. This is the only
        // settings channel Zed can drive, so it must be sufficient on
        // its own. The shape mirrors ruff-vscode's: an array of
        // per-folder settings plus a global fallback.
        let server_settings: ServerSettings = params
            .initialization_options
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Use the first folder's settings, falling back to the global
        // settings. The root-level folder_settings is used as the single-root
        // fallback; per-folder settings are resolved through FolderAnalysisContext.
        let folder_settings = server_settings
            .settings
            .first()
            .cloned()
            .unwrap_or_else(|| server_settings.global_settings.clone());

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

        // Build per-folder analysis contexts. Each workspace
        // folder (or `root_uri` when no folders are supplied) gets its own
        // config, editor settings, and local typesheds. This is the
        // single longest-prefix ownership operation for every channel.
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
        // Clone this server's per-instance compile counter so
        // the free-function builders below increment it instead of a process
        // global. Parallel test servers keep independent tallies.
        let filter_compile_count = self.state.lock().await.filter_compile_count.clone();
        let folder_contexts = {
            let filter_compile_count = Arc::clone(&filter_compile_count);
            tokio::task::spawn_blocking(move || {
                build_folder_contexts(
                    root_clone.as_deref(),
                    &ws_folder_paths,
                    &server_settings_clone,
                    &filter_compile_count,
                )
            })
            .await
            .unwrap_or_default()
        };

        // Load the root-level config and stubs for single-root fallback.
        let root_clone2 = root.clone();
        let user_stubs =
            tokio::task::spawn_blocking(move || load_workspace_stubs(root_clone2.as_deref()))
                .await
                .unwrap_or_else(|_| Arc::new(std::collections::BTreeMap::new()));
        let file_config = root
            .as_deref()
            .and_then(|r| ry_config::Config::load_from_dir(r).ok().flatten())
            .unwrap_or_default();

        // Cache the root-level baseline so the fallback publish
        // path (documents outside every folder root) performs no disk access.
        // Built outside the lock alongside `file_config`.
        let root_baseline =
            match load_folder_baseline(&folder_settings, &file_config, root.as_deref()) {
                Ok(opt) => opt,
                Err(error) => {
                    tracing::warn!(%error, "failed to load root baseline; no baseline cached");
                    None
                }
            };

        // Initialize per-folder project caches.
        let folder_projects: HashMap<String, Arc<Mutex<ProjectCache>>> = folder_contexts
            .iter()
            .map(|ctx| {
                (
                    ctx.root.to_string_lossy().to_string(),
                    Arc::new(Mutex::new(ProjectCache::default())),
                )
            })
            .collect();

        // Precompute the root-level filter alongside the per-folder ones, so
        // the publish loop borrows compiled values and never recompiles.
        let (root_filter, root_min_confidence, root_excludes) =
            compute_folder_filter(&file_config, &folder_settings, &filter_compile_count);

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
        state.supports_did_change_watched_files = supports_did_change_watched_files;
        // Install per-folder analysis contexts and project caches.
        state.folder_contexts = folder_contexts;
        state.folder_projects = folder_projects;
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
                // Quick fixes that insert `# ry: ignore[CODE]` line
                // suppressions, plus a file-level `# ry: ignore-file` action.
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                // Advertise workspace folder support so clients send
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

        // If the client supports workspace/configuration, pull the
        // `ry.*` section now. This is the primary settings path for VS
        // Code and supersedes whatever was in initializationOptions.
        let should_pull = {
            let state = self.state.lock().await;
            state.supports_workspace_configuration
        };
        if should_pull {
            // Pull per-folder settings and recompute the cached filter /
            // min_confidence / excludes through the shared
            // `pull_folder_settings` helper (also used by
            // `did_change_configuration`) so the two pull paths cannot drift.
            self.pull_folder_settings().await;
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

        // Spawn a background indexer to discover and parse all .R/.r
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
        // Incremental sync: process each change event, applying
        // range-based edits to the old text and building a tree-sitter
        // InputEdit for incremental reparse.
        //
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
        // test-only scheduling seam. Signals that the
        // document version has been bumped and diagnostics re-scheduled,
        // so a waiting test can release the parse barrier knowing the
        // version-stamped cache will reject the stale parse. When no test
        // is waiting this is a single atomic swap — effectively free.
        crate::test_seam::note_did_change();
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // Configuration changed. If the client supports pull
        // configuration, re-pull the `ry.*` section. Otherwise parse
        // the settings blob sent in the notification.
        let should_pull = {
            let state = self.state.lock().await;
            state.supports_workspace_configuration
        };

        if should_pull {
            // Pull per-folder settings and recompute the cached filter /
            // min_confidence / excludes through the shared
            // `pull_folder_settings` helper (also used by `initialized`)
            // so the two pull paths cannot drift.
            self.pull_folder_settings().await;
        } else {
            // The client sent settings inline. They may be wrapped in
            // an outer "ry" key (VS Code) or be the raw ry settings.
            let raw = &params.settings;
            let ry_section = raw.get("ry").unwrap_or(raw);
            if let Ok(settings) = serde_json::from_value::<FolderSettings>(ry_section.clone()) {
                let mut state = self.state.lock().await;
                state.folder_settings = settings.clone();
                // Propagate to folder contexts.
                for ctx in &mut state.folder_contexts {
                    ctx.folder_settings = settings.clone();
                }
                state.server_settings.global_settings = settings;
                // Recompute the cached filter / min_confidence / excludes
                // through the shared `refresh_cached_folder_filters` helper
                // so the push and pull paths share one refresh and cannot
                // drift. This stays out of `publish_diagnostics`: the
                // publish-cycle contract asserts zero filter compilations
                // *during a publish cycle*.
                refresh_cached_folder_filters(&mut state);
            }
        }

        self.spawn_background_index().await;

        // Republish diagnostics for every open document so the new
        // settings take effect immediately.
        self.republish_all_open_documents().await;
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        // Rebuild the sorted folder contexts, load contexts and
        // start indexing for added roots, remove all state owned by removed
        // roots, then republish only after the new state is installed.
        let removed_uris: Vec<Url> = params.event.removed.iter().map(|f| f.uri.clone()).collect();
        let removed_paths: Vec<PathBuf> = removed_uris
            .iter()
            .filter_map(|uri| uri.to_file_path().ok())
            .collect();

        // URIs for open documents owned by removed roots — diagnostics for
        // these must be cleared.
        let (docs_to_clear, docs_to_republish): (Vec<Url>, Vec<Url>) = {
            let state = self.state.lock().await;

            // Build added folder contexts through the same
            // `build_folder_contexts` path used at initialize, so dynamically
            // added folders get proper per-folder settings and config.
            let docs_to_clear: Vec<Url> = state
                .docs
                .keys()
                .filter(|p| {
                    removed_paths
                        .iter()
                        .any(|r| std::path::Path::new(p.as_str()).starts_with(r))
                })
                .map(|p| path_to_uri(p))
                .collect();
            let docs_to_republish: Vec<Url> = state
                .docs
                .keys()
                .filter(|p| {
                    !removed_paths
                        .iter()
                        .any(|r| std::path::Path::new(p.as_str()).starts_with(r))
                })
                .map(|p| path_to_uri(p))
                .collect();
            (docs_to_clear, docs_to_republish)
        };

        {
            let mut state = self.state.lock().await;

            // step 3: Remove disk_files, trees, diagnostics, and
            // contexts owned by removed roots BEFORE rebuilding, so stale
            // state never enters the next check.
            state.disk_files.retain(|p, _| {
                !removed_paths
                    .iter()
                    .any(|r| std::path::Path::new(p.as_str()).starts_with(r))
            });
            state.trees.retain(|p, _| {
                !removed_paths
                    .iter()
                    .any(|r| std::path::Path::new(p.as_str()).starts_with(r))
            });
            state.parsed.retain(|p, _| {
                !removed_paths
                    .iter()
                    .any(|r| std::path::Path::new(p.as_str()).starts_with(r))
            });
            state.scopes.retain(|p, _| {
                !removed_paths
                    .iter()
                    .any(|r| std::path::Path::new(p.as_str()).starts_with(r))
            });

            // step 1: Rebuild the sorted folder contexts from
            // the surviving + added roots, using the shared builder so
            // added folders get per-folder settings and config.
            let added_paths_ref: Vec<(usize, PathBuf)> = params
                .event
                .added
                .iter()
                .enumerate()
                .filter_map(|(idx, f)| f.uri.to_file_path().ok().map(|path| (idx, path)))
                .collect();

            // Build new contexts for added folders through build_folder_contexts.
            let new_contexts = if !added_paths_ref.is_empty() {
                let server_settings = state.server_settings.clone();
                // thread this server's compile counter in so
                // added-folder filter builds count toward it, not a global.
                let filter_compile_count = Arc::clone(&state.filter_compile_count);
                tokio::task::spawn_blocking(move || {
                    build_folder_contexts(
                        None,
                        &added_paths_ref
                            .iter()
                            .map(|(_, path)| (usize::MAX, path.clone()))
                            .collect::<Vec<_>>(),
                        &server_settings,
                        &filter_compile_count,
                    )
                })
                .await
                .unwrap_or_default()
            } else {
                Vec::new()
            };

            // Remove contexts for removed roots.
            state
                .folder_contexts
                .retain(|ctx| !removed_paths.iter().any(|p| p == &ctx.root));
            // Add new contexts.
            state.folder_contexts.extend(new_contexts);
            // Sort by root path length descending for longest-prefix matching.
            state
                .folder_contexts
                .sort_by_key(|ctx| std::cmp::Reverse(ctx.root.as_os_str().len()));

            // Per-folder project caches: remove for deleted roots, add for new.
            for p in &removed_paths {
                state
                    .folder_projects
                    .remove(&p.to_string_lossy().to_string());
            }
            let ctx_keys: Vec<String> = state
                .folder_contexts
                .iter()
                .map(|ctx| ctx.root.to_string_lossy().to_string())
                .collect();
            for key in &ctx_keys {
                state
                    .folder_projects
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(ProjectCache::default())));
            }

            // step 5: Bump index generation so results from the
            // old folder set are discarded.
            state.index_generation = state.index_generation.wrapping_add(1);
        }

        // step 2: Load contexts and start indexing for the new
        // folder set. Skip when no folder contexts remain (all removed)
        // so state.root does not re-index a removed directory.
        let has_contexts = !self.state.lock().await.folder_contexts.is_empty();
        if has_contexts {
            self.spawn_background_index().await;
        }

        // step 3 continued: Clear diagnostics for documents owned
        // by removed roots.
        for uri in &docs_to_clear {
            self.client
                .publish_diagnostics(uri.clone(), Vec::new(), None)
                .await;
        }

        // step 4: Republish only after the new state is installed. Not
        // `republish_all_open_documents`: documents under removed roots
        // were just cleared above and must stay cleared. They fall back
        // to root-level eligibility, so a blanket reschedule would
        // re-publish check results for them.
        for uri in &docs_to_republish {
            self.schedule_diagnostics(uri.clone()).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        // Refresh configuration and filesystem-backed resolution when any
        // registered package metadata, data, stub, or config file changes.
        let config_or_baseline_changed = params.changes.iter().any(|change| {
            let path = change.uri.path();
            path.ends_with("ry.toml") || path.ends_with(".json")
        });
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

        // A config (`ry.toml`) or baseline (`.json`) change
        // requires rebuilding each folder's effective config + baseline.
        // The new contexts are built OUTSIDE the write lock (all disk I/O
        // happens in `rebuild_folder_contexts`), then atomically swapped in.
        // A failed reload retains the last valid value for each field and
        // emits a visible warning; it never silently clears the baseline.
        if config_or_baseline_changed {
            let (old_contexts, root, filter_compile_count) = {
                let state = self.state.lock().await;
                // carry this server's compile counter into the
                // rebuild so reloaded filters count toward it, not a global.
                (
                    state.folder_contexts.clone(),
                    state.root.clone(),
                    Arc::clone(&state.filter_compile_count),
                )
            };
            let old_contexts_for_task = old_contexts.clone();
            // spawn_blocking keeps every disk read off the async runtime.
            let new_contexts = match tokio::task::spawn_blocking(move || {
                rebuild_folder_contexts(&old_contexts_for_task, &filter_compile_count)
            })
            .await
            {
                Ok(new_contexts) => new_contexts,
                Err(error) => {
                    tracing::warn!(%error, "folder context reload task failed; retaining previous contexts");
                    old_contexts
                }
            };
            {
                let mut state = self.state.lock().await;
                state.folder_contexts = new_contexts.clone();
                // Sync the root-level fallback state from the rebuilt
                // contexts so the fallback paths see the new
                // config/baseline.
                for ctx in &new_contexts {
                    if state.root.as_deref() == Some(ctx.root.as_path()) {
                        state.file_config = ctx.config.clone();
                        state.root_baseline = ctx.baseline.clone();
                        // Keep the root-level fallback filter in step with the
                        // reloaded root config.
                        state.root_filter = ctx.filter.clone();
                        state.root_min_confidence = ctx.min_confidence;
                        state.root_excludes = ctx.excludes.clone();
                    }
                }
                tracing::info!("workspace config/baseline reloaded");
            }

            // Root-level typesheds feed the distinct user-stub analysis
            // channel; reload them off the async runtime too.
            if let Some(root) = root
                && let Ok(stubs) =
                    tokio::task::spawn_blocking(move || load_workspace_stubs(Some(&root))).await
            {
                let mut state = self.state.lock().await;
                state.user_stubs = stubs;
            }
        }

        self.spawn_background_index().await;

        // Republish diagnostics for every open document so the new
        // config/baseline takes effect immediately.
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
            state.scopes.remove(&path);
            state.trees.remove(&path);
            // Bump the generation so any in-flight diagnostics publish
            // queued while the file was open is invalidated.
            state.diag_generation = state.diag_generation.wrapping_add(1);
            state.docs.keys().cloned().collect::<Vec<_>>()
        };
        {
            // Clean up the file in both the root project cache and the
            // owning folder's per-folder cache.
            let (root_project, folder_project_opt) = {
                let state = self.state.lock().await;
                let root = Arc::clone(&state.project);
                let folder = state.folder_context_for_path(&path).and_then(|ctx| {
                    state
                        .folder_projects
                        .get(&ctx.root.to_string_lossy().to_string())
                        .cloned()
                });
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

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.clone();
        let path = uri_to_path(&uri);
        let range = params.range;

        // Parse the document (cached). On any parse
        // failure we return `None` (no hints) rather than erroring, so
        // the editor simply shows nothing instead of a broken state.
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
}

impl Backend {
    /// Apply a single incremental text change.
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
    ) -> bool {
        if let Some(range) = change.range {
            // Incremental: apply the range edit to the old text.
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
            } else {
                // No old text — treat as full replacement. Clear stale tree.
                {
                    let mut state = self.state.lock().await;
                    state.trees.remove(path);
                }
                self.update_doc(path.to_string(), change.text, version)
                    .await;
            }
            true
        } else {
            // Full text change (no range). Clear stale tree.
            {
                let mut state = self.state.lock().await;
                state.trees.remove(path);
            }
            self.update_doc(path.to_string(), change.text, version)
                .await;
            true
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

    /// Return the current AST for `path` together with the exact source
    /// text it was parsed from. Pairing the two is atomic: handlers use
    /// the text for byte-offset / UTF-16 conversions that must match the
    /// AST's span offsets, so a concurrent `didChange` racing the parse
    /// can never yield a stale text applied to a fresher AST (or vice
    /// versa). The parse cache is read and repopulated under the state
    /// lock; parsing itself (which needs a non-`Send` `RParser`) happens
    /// outside it.
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
            // Incremental reparse when an old tree exists (`tree_for`
            // already enforces the version match).
            let old_tree = {
                let state = self.state.lock().await;
                state.tree_for(path)
            };
            // test-only scheduling barrier. When armed, the
            // parse pauses here — after reading the document text, version,
            // and old tree, but before the expensive parse — so the test can
            // force the interleaving:
            //   1. parse version N starts (we are here)
            //   2. didChange installs N+1
            //   3. parse N finishes (test releases the barrier)
            //   4. stale result is rejected by store_tree / record_parse
            //   5. the retry loop parses the current version N+1 fresh
            // The seam controls scheduling only; cache policy is production
            // code. When not armed this is a single relaxed atomic load.
            crate::test_seam::maybe_pause().await;
            let mut parser = RParser::new().ok()?;
            let file = if let Some(tree) = old_tree {
                let (parsed, new_tree) = parser.parse_with_tree(path, &text, Some(&tree)).ok()?;
                // Store only if the version still matches (`store_tree`).
                {
                    let mut state = self.state.lock().await;
                    state.store_tree(path, version, new_tree);
                }
                Arc::new(parsed)
            } else {
                let (parsed, new_tree) = parser.parse_with_tree(path, &text, None).ok()?;
                {
                    let mut state = self.state.lock().await;
                    state.store_tree(path, version, new_tree);
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
    /// Used by inlay_hint so it doesn't re-run the check on
    /// every request. Returns `None` when the document
    /// is not open or parsing fails.
    async fn scope_for(&self, path: &str) -> Option<ry_checker::Scope> {
        // Fast path: cached scope with matching version.
        {
            let state = self.state.lock().await;
            if let Some(version) = state.versions.get(path).copied()
                && let Some((cached_v, scope)) = state.scopes.get(path)
                && *cached_v == version
            {
                return Some(scope.clone());
            }
        }
        // Cache miss: parse (via the parse cache) + check, then store.
        let (file, _) = self.parsed_file(path).await?;
        let mut checker = ry_checker::Checker::new(path);
        let user_stubs = {
            let state = self.state.lock().await;
            // Use the owning folder's stubs for single-file checks.
            state
                .folder_context_for_path(path)
                .map(|ctx| Arc::clone(&ctx.stubs))
                .unwrap_or_else(|| Arc::clone(&state.user_stubs))
        };
        checker.set_user_stubs(user_stubs);
        let (_, scope) = checker.check_with_scope(&file);
        // Cache only when the parse behind this scope is still current.
        // One post-check suffices: the parse cache still holds `file`
        // (Arc identity) tagged with a version equal to the live
        // document version. `record_parse` stamps that pairing at store
        // time, and a superseded entry can never re-install an older
        // `Arc` (every miss parses into a fresh `Arc`), so checking
        // before the check run would only re-derive what this check
        // already proves.
        {
            let mut state = self.state.lock().await;
            let current_version = state.parsed.get(path).and_then(|(cached_version, cached)| {
                (Arc::ptr_eq(cached, &file)
                    && state.versions.get(path).copied() == Some(*cached_version))
                .then_some(*cached_version)
            });
            if let Some(version) = current_version {
                state
                    .scopes
                    .insert(path.to_string(), (version, scope.clone()));
            }
        }
        Some(scope)
    }

    /// Pull `ry` settings per folder scope via
    /// `workspace/configuration`, install each result into the matching
    /// [`FolderAnalysisContext`]'s `folder_settings`, update the root-level
    /// fallback, then recompute the cached filter / min_confidence /
    /// excludes through [`refresh_cached_folder_filters`].
    ///
    /// Shared by the initial pull in [`Backend::initialized`] and the
    /// refresh pull in [`Backend::did_change_configuration`] so the two
    /// paths cannot drift. One item is sent per folder root (scoped to that
    /// root); a final root-scoped item updates the server-wide fallback.
    /// The recompute stays here — outside `publish_diagnostics` —
    /// preserving the publish-cycle contract of zero filter compilations
    /// during a publish cycle.
    async fn pull_folder_settings(&self) {
        // One item per folder root scope, then a root-scoped item for the
        // server-wide fallback. Built under the lock, then sent without it.
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
        // Per-folder results: install into the matching context by index
        // (the items were built in `folder_contexts` order).
        for (idx, ctx) in state.folder_contexts.iter_mut().enumerate() {
            if let Some(value) = values.get(idx)
                && let Ok(settings) = serde_json::from_value::<FolderSettings>(value.clone())
            {
                ctx.folder_settings = settings;
            }
        }
        // Root-scoped result (the item after the per-folder entries) updates
        // the server-wide fallback.
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
    async fn publish_diagnostics(&self, uri: Url, generation: u64) {
        // Snapshot the open docs under the lock, then drop the lock
        // before running the checker so a slow check doesn't block
        // other LSP requests (e.g. didOpen of a second file).
        //
        // also clone this server's per-instance counters so the
        // compile-count delta measured below is scoped to this server, not the
        // process. `publish_start_count` snapshots the counter at cycle start.
        // Only the eligible documents' versions are snapshotted, not the
        // whole `versions` map.
        let (path, doc_versions, filter_compile_count, compile_during_last_publish) = {
            let state = self.state.lock().await;
            (
                uri_to_path(&uri),
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
                Arc::clone(&state.filter_compile_count),
                Arc::clone(&state.compile_during_last_publish),
            )
        };
        // snapshot the compile counter at the start of this
        // publish cycle so we can measure compilations during it.
        let publish_start_count = filter_compile_count.load(std::sync::atomic::Ordering::Relaxed);
        let requested_is_eligible = {
            let state = self.state.lock().await;
            state.eligibility_for_path(&path)
        };
        if !requested_is_eligible {
            self.client
                .publish_diagnostics(uri.clone(), Vec::new(), None)
                .await;
        }

        let mut project_files = Vec::with_capacity(doc_versions.len());
        for (doc_path, version) in &doc_versions {
            let Some((file, _)) = self.parsed_file(doc_path).await else {
                continue;
            };
            project_files.push((doc_path.clone(), *version, file));
        }
        // Disk files never shadow open documents, and files in disabled
        // folders are dropped by the same eligibility rule as open ones.
        // Each entry is cloned once, straight into the check input.
        let disk_entries: Vec<(String, i32, Arc<SourceFile>)> = {
            let state = self.state.lock().await;
            let open_paths: std::collections::HashSet<&str> =
                project_files.iter().map(|(p, _, _)| p.as_str()).collect();
            state
                .disk_files
                .iter()
                .filter(|(p, _)| state.eligibility_for_path(p))
                .filter(|(p, _)| !open_paths.contains(p.as_str()))
                .map(|(p, file)| (p.clone(), 0, Arc::clone(file)))
                .collect()
        };
        project_files.extend(disk_entries);

        // Partition files by owning folder and check each folder
        // independently so two roots defining the same package differently
        // never collide. Each folder gets its own ProjectCache, stubs,
        // and workspace context.
        let (folder_contexts, folder_project_handles, root_project, user_stubs) = {
            let state = self.state.lock().await;
            (
                state.folder_contexts.clone(),
                state.folder_projects.clone(),
                Arc::clone(&state.project),
                Arc::clone(&state.user_stubs),
            )
        };

        // Partition project_files by folder root (longest-prefix ownership).
        // Files not owned by any folder go to the root project.
        use std::collections::BTreeMap;
        let mut per_folder: BTreeMap<String, FolderPartition> = BTreeMap::new();
        for (fp, ver, file) in project_files {
            if let Some(ctx) = folder_contexts
                .iter()
                .find(|c| std::path::Path::new(&fp).starts_with(&c.root))
            {
                let key = ctx.root.to_string_lossy().to_string();
                per_folder
                    .entry(key)
                    .or_insert_with(|| (Some(ctx.clone()), Vec::new()))
                    .1
                    .push((fp, ver, file));
            } else {
                per_folder
                    .entry("__root__".to_string())
                    .or_insert_with(|| (None, Vec::new()))
                    .1
                    .push((fp, ver, file));
            }
        }

        // Check each folder partition independently. `per_folder` is
        // consumed: each partition's files go into its check by value.
        let mut all_results: Vec<ProjectCheckResult> = Vec::new();
        for (key, (ctx_opt, files)) in per_folder {
            let (stubs, workspace_context, project_handle) = match ctx_opt {
                Some(ctx) => (
                    Arc::clone(&ctx.stubs),
                    ctx.workspace_context.clone(),
                    folder_project_handles
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| Arc::clone(&root_project)),
                ),
                None => (Arc::clone(&user_stubs), None, Arc::clone(&root_project)),
            };
            let mut project = project_handle.lock().await;
            let result = project.check_with_workspace(files, stubs, workspace_context.as_ref());
            all_results.push(result);
        }

        // An edit that arrived while parsing/checking invalidates this whole
        // project result because every open document is republished below.
        {
            let state = self.state.lock().await;
            if state.diag_generation != generation {
                return;
            }
        }

        // Publish diagnostics for every checked file, applying per-folder
        // filter/confidence/exclude/baseline state. Results are consumed:
        // each file's diagnostic vec is taken, not cloned.
        for result in all_results {
            let ProjectCheckResult {
                diagnostics: per_file,
                files: checked_files,
            } = result;
            for (diagnostic_path, mut diagnostics) in per_file {
                // Use the precomputed filter/confidence/excludes
                // from the FolderAnalysisContext instead of reconstructing
                // them per file inside the publish loop.
                let (filter, min_confidence, excludes, baseline, folder_root) = {
                    let state = self.state.lock().await;
                    let ctx = state.folder_context_for_path(&diagnostic_path);
                    let (filter, min_confidence, excludes) = match ctx {
                        Some(c) => (c.filter.clone(), c.min_confidence, c.excludes.clone()),
                        None => {
                            // Fallback for files outside every folder root:
                            // borrow the precomputed *root-level* values, the
                            // same source the `baseline` and `folder_root`
                            // fallbacks below use. Borrowing an arbitrary
                            // folder's values here applied unrelated severity
                            // and exclude rules. Precomputed, so the publish
                            // loop still performs no filter compilation.
                            (
                                state.root_filter.clone(),
                                state.root_min_confidence,
                                state.root_excludes.clone(),
                            )
                        }
                    };
                    let baseline = state.effective_baseline_for_path(&diagnostic_path);
                    let folder_root = ctx
                        .map(|c| Some(c.root.clone()))
                        .unwrap_or_else(|| state.root.clone());
                    (filter, min_confidence, excludes, baseline, folder_root)
                };
                ry_checker::apply_filter_to_diagnostics(&mut diagnostics, &filter);

                if let Some(min) = min_confidence {
                    diagnostics.retain(|d| d.confidence >= min);
                }

                if !excludes.is_empty() {
                    let rel = ry_config::diagnostic_path(&diagnostic_path, folder_root.as_deref());
                    if excludes.matches(&rel) {
                        continue;
                    }
                }

                if let Some(ref baseline) = baseline {
                    ry_config::subtract_baseline(
                        &mut diagnostics,
                        baseline,
                        folder_root.as_deref(),
                    );
                }

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
        // record how many filter compilations happened during
        // this publish cycle. Must be zero with precomputation.
        let publish_end_count = filter_compile_count.load(std::sync::atomic::Ordering::Relaxed);
        compile_during_last_publish.store(
            publish_end_count - publish_start_count,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Discover and parse all `.R`/`.r` files under the workspace root(s)
    /// in a background task. Results are stored in `state.disk_files` and a
    /// diagnostic refresh is triggered so cross-file calls into unopened
    /// files resolve on the next check.
    async fn spawn_background_index(&self) {
        let (roots_with_config, index_gen) = {
            let mut state = self.state.lock().await;
            // Bump the index generation so results from a prior
            // folder set are discarded when they arrive.
            state.index_generation = state.index_generation.wrapping_add(1);
            let idx_gen = state.index_generation;
            let roots = if !state.folder_contexts.is_empty() {
                // Use per-folder stubs for workspace resolution.
                state
                    .folder_contexts
                    .iter()
                    .map(|ctx| (ctx.root.clone(), ctx.config.clone(), Arc::clone(&ctx.stubs)))
                    .collect()
            } else if let Some(root) = &state.root {
                vec![(
                    root.clone(),
                    state.file_config.clone(),
                    Arc::clone(&state.user_stubs),
                )]
            } else {
                Vec::new()
            };
            (roots, idx_gen)
        };
        if roots_with_config.is_empty() {
            return;
        }

        let indexed = tokio::task::spawn_blocking(move || {
            let mut all_disk_files: HashMap<String, Arc<SourceFile>> = HashMap::new();
            let mut contexts = Vec::new();
            let mut all_truncated: Vec<(PathBuf, ry_workspace::TruncationReport)> = Vec::new();
            for (root, config, stubs) in &roots_with_config {
                let outcome = crate::index::index_workspace(root, config);
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
                // emit a structured tracing event and one
                // user-visible LSP warning per scan generation when a cap
                // is hit. A cap hit is never silent.
                for (root, report) in &truncated {
                    if report.max_files_hit {
                        tracing::warn!(
                            root = %root.display(),
                            cap = "index.max-files",
                            omitted = report.omitted_count(),
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
                // step 5: Discard results from an index generation
                // belonging to the old folder set.
                if state.index_generation != index_gen {
                    tracing::debug!(
                        gen = index_gen,
                        current = state.index_generation,
                        "discarding stale background index results"
                    );
                    return;
                }
                state.disk_files = disk_files;
                // Update each folder context's workspace_context
                // with the freshly resolved package metadata.
                for ctx in &mut state.folder_contexts {
                    if let Some((_, wc)) = contexts.iter().find(|(root, _)| root == &ctx.root) {
                        ctx.workspace_context = Some(wc.clone());
                    }
                }
                // one user-visible warning per scan generation.
                if cap_hit {
                    let _ = self
                        .client
                        .log_message(
                            tower_lsp::lsp_types::MessageType::WARNING,
                            "ry: discovery cap reached; some R files were not indexed. See server logs for details (index.max-files / index.max-file-bytes / index.max-depth).",
                        )
                        .await;
                }
            }
            Err(error) => tracing::warn!(%error, "background workspace index failed"),
        }
    }

    /// Schedule a diagnostics republish for every open document. Used
    /// after a global state change (settings, watched files) so the new
    /// state takes effect immediately without waiting for an edit.
    async fn republish_all_open_documents(&self) {
        let open_uris: Vec<Url> = {
            let state = self.state.lock().await;
            state.docs.keys().map(|p| path_to_uri(p)).collect()
        };
        for uri in open_uris {
            self.schedule_diagnostics(uri).await;
        }
    }

    /// Debounce diagnostics for `uri`: bump the workspace generation counter
    /// and spawn a task that sleeps ~180ms, then publishes diagnostics
    /// ONLY if its generation is still the latest. A newer edit during the
    /// sleep window bumps the counter and the stale task aborts, so
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

/// Load stubs directly from a loaded config's typeshed directories.
/// Used per-folder so two roots defining the same package differently
/// never collide.
fn load_stubs_from_config(
    config: &ry_config::Config,
) -> Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>> {
    let mut merged = std::collections::BTreeMap::new();
    for dir in &config.typeshed {
        match ry_typeshed::load_stub_dir_with_warnings(dir) {
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

/// Discover the effective `ry.toml` config for a folder.
///
/// When the editor supplies a `configuration` override it is resolved
/// relative to `folder_root` (unless absolute) and loaded directly; on
/// failure the code falls back to directory discovery. Returns
/// `Ok(default)` when no `ry.toml` is found, and `Err` only when discovery
/// itself fails (I/O or parse error) so callers can decide whether to
/// retain a previous config. Disk I/O happens here — callers MUST run this
/// outside the state lock.
fn discover_folder_config(
    folder_settings: &FolderSettings,
    folder_root: &std::path::Path,
) -> std::result::Result<ry_config::Config, ry_config::ConfigError> {
    if let Some(config_rel) = &folder_settings.configuration {
        let config_path = if PathBuf::from(config_rel).is_absolute() {
            PathBuf::from(config_rel)
        } else {
            folder_root.join(config_rel)
        };
        match ry_config::Config::load_file(&config_path) {
            Ok(cfg) => return Ok(cfg),
            Err(error) => {
                tracing::warn!(
                    path = %config_path.display(),
                    %error,
                    "failed to load configuration override; falling back to discovery"
                );
                // Fall back to directory discovery (walks up the tree).
                return Ok(ry_config::Config::discover(folder_root)
                    .ok()
                    .flatten()
                    .map(|(_, c)| c)
                    .unwrap_or_default());
            }
        }
    }
    Ok(ry_config::Config::discover(folder_root)
        .ok()
        .flatten()
        .map(|(_, c)| c)
        .unwrap_or_default())
}

/// Resolve the baseline path from editor settings / `ry.toml`
/// and load it from disk.
///
/// Returns `Ok(None)` when no baseline is configured. `Err` signals a
/// configured-but-unloadable baseline (missing file, corrupt JSON, wrong
/// version) so reload callers can retain the last valid value rather than
/// silently clearing it. Disk I/O happens here — callers MUST run this
/// outside the state lock.
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
    BASELINE_DISK_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ry_config::load_baseline(&resolved)
        .map(Some)
        .map_err(|e| format!("{e}"))
}

/// Build per-folder analysis contexts for every workspace root.
///
/// Each context holds the effective `ry.toml` config (from directory
/// discovery or the editor `configuration` override), the matching
/// editor [`FolderSettings`], local typesheds loaded from that
/// folder's config, and the cached baseline. When
/// `workspace_folders` is empty, `root_uri` becomes the single folder.
fn build_folder_contexts(
    root: Option<&std::path::Path>,
    workspace_folders: &[(usize, PathBuf)],
    server_settings: &ServerSettings,
    filter_count: &std::sync::atomic::AtomicU64,
) -> Vec<FolderAnalysisContext> {
    // Determine the folder roots: workspace_folders when provided, else root_uri.
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

        // Discover config (defaulting on failure) and
        // load the baseline once into the context so the publish path never
        // touches disk.
        let config =
            discover_folder_config(&folder_settings, folder_root).unwrap_or_else(|error| {
                tracing::warn!(
                    root = %folder_root.display(),
                    %error,
                    "failed to load folder config; using default config"
                );
                ry_config::Config::default()
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

        // Load stubs from this folder's config so two roots
        // defining the same package differently are isolated.
        let stubs = load_stubs_from_config(&config);

        // Precompute filter/min_confidence/excludes once
        // per folder so the publish loop performs one ownership lookup
        // and borrows the compiled values.
        let (filter, min_confidence, excludes) =
            compute_folder_filter(&config, &folder_settings, filter_count);
        contexts.push(FolderAnalysisContext {
            root: folder_root.clone(),
            config,
            folder_settings,
            stubs,
            workspace_context: None,
            baseline,
            filter,
            min_confidence,
            excludes,
        });
    }

    // Longest-prefix ownership: sort by root path length descending.
    contexts.sort_by_key(|ctx| std::cmp::Reverse(ctx.root.as_os_str().len()));
    contexts
}

/// Rebuild a single folder's analysis context from disk.
///
/// Each field is reloaded independently. On any sub-failure (config parse,
/// baseline parse) the **last valid value for that field is retained** and a
/// visible warning emitted — a corrupt reload never silently clears the
/// baseline. `folder_settings` and `workspace_context` are not reloaded by
/// file-watch events (they come from editor push / the background indexer
/// respectively) and are carried over unchanged. Disk I/O happens here;
/// callers MUST run this outside the state lock.
fn rebuild_folder_context(
    old: &FolderAnalysisContext,
    filter_count: &std::sync::atomic::AtomicU64,
) -> FolderAnalysisContext {
    // Reload config; on failure retain the previous config.
    let config = match discover_folder_config(&old.folder_settings, &old.root) {
        Ok(cfg) => cfg,
        Err(error) => {
            tracing::warn!(
                root = %old.root.display(),
                %error,
                "failed to reload folder config; retaining previous config"
            );
            old.config.clone()
        }
    };
    // Reload stubs from the (possibly retained) config. If the reload
    // returns fewer stubs than before (e.g. a malformed stub directory),
    // retain the previous stubs to avoid silently degrading analysis.
    let new_stubs = load_stubs_from_config(&config);
    let stubs = if new_stubs.is_empty() && !old.stubs.is_empty() {
        tracing::warn!(
            root = %old.root.display(),
            "stub reload returned empty; retaining previous stubs"
        );
        old.stubs.clone()
    } else {
        new_stubs
    };
    // Reload baseline; on failure retain the last valid baseline.
    let baseline = match load_folder_baseline(&old.folder_settings, &config, Some(&old.root)) {
        Ok(opt) => opt,
        Err(error) => {
            tracing::warn!(
                root = %old.root.display(),
                %error,
                "failed to reload baseline; retaining last valid baseline"
            );
            old.baseline.clone()
        }
    };
    // Recompute the precomputed filter/excludes from the
    // reloaded config.
    let (filter, min_confidence, excludes) =
        compute_folder_filter(&config, &old.folder_settings, filter_count);
    FolderAnalysisContext {
        root: old.root.clone(),
        config,
        folder_settings: old.folder_settings.clone(),
        stubs,
        workspace_context: old.workspace_context.clone(),
        baseline,
        filter,
        min_confidence,
        excludes,
    }
}

/// Rebuild every folder's analysis context from disk,
/// returning a replacement `Vec` in the same order as `old`. Used by the
/// watched-files handler to rebuild outside the write lock and then swap
/// atomically. Each context is rebuilt independently; a single folder's
/// reload failure does not affect the others.
fn rebuild_folder_contexts(
    old: &[FolderAnalysisContext],
    filter_count: &std::sync::atomic::AtomicU64,
) -> Vec<FolderAnalysisContext> {
    old.iter()
        .map(|ctx| rebuild_folder_context(ctx, filter_count))
        .collect()
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
