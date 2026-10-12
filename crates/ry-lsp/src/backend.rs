//! LSP backend: `Backend`, `State`, the `LanguageServer` impl, and the
//! document cache / debounce machinery.

mod handlers;
mod reconciliation;

use reconciliation::{
    RECONCILE_ROUNDS_BEFORE_PACING, ReconcileRoundProgress, Reconciliation, RefreshDuty,
};

use crate::diagnostics::{
    diagnostic_origin, diagnostic_to_lsp_with_source, make_ignore_action, make_ignore_file_action,
};
use crate::hints::collect_inlay_hints;
use crate::positions::{byte_offset_to_point, position_to_byte_offset};
use crate::settings::{FolderSettings, ServerSettings};
#[cfg(feature = "test-util")]
use crate::test_seam::{Counter, Seam};

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
/// only baseline disk-read site in the LSP, so tests can assert the
/// publish path performs none.
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

#[derive(Clone)]
struct OpenSource {
    native: Option<PathBuf>,
    // Only populated while multiple native URIs share one display key.
    // The active buffer also lives in `docs`; inactive buffers need this
    // snapshot so a close or incremental edit can restore their text.
    shadow: Option<(String, i32)>,
}

/// The original URIs open at a display path, in a stable order. A path
/// without open buffers publishes at its display URI.
fn source_uris(
    open_source_paths: &HashMap<String, HashMap<Url, OpenSource>>,
    path: &str,
) -> Vec<Url> {
    let mut uris: Vec<Url> = open_source_paths
        .get(path)
        .map(|open| open.keys().cloned().collect())
        .unwrap_or_default();
    if uris.is_empty() {
        return vec![path_to_uri(path)];
    }
    uris.sort();
    uris
}

#[derive(Default)]
pub(super) struct State {
    /// Open documents: path -> current source text. Every open document
    /// joins the multi-file project so cross-file resolution sees unsaved
    /// edits.
    docs: HashMap<String, String>,
    /// Native file paths and original URIs for open buffers. The document
    /// map is keyed by a display string, which can collapse distinct Unix
    /// filenames; annotation adoption must retain the identity before that
    /// conversion. Multiple URIs at one display key are ambiguous.
    open_source_paths: HashMap<String, HashMap<Url, OpenSource>>,
    /// The URI whose snapshot currently occupies `docs` at a colliding key.
    active_collision_uri: HashMap<String, Url>,
    /// path -> version of the most recent edit.
    versions: HashMap<String, i32>,
    /// path -> (version, parsed SourceFile), valid only while the version
    /// matches `versions`.
    parsed: HashMap<String, (i32, Arc<SourceFile>)>,
    /// Hints belong to one document version and loaded stub snapshot.
    hints: HashMap<String, CachedHints>,
    /// Workspace-wide debounce generation; see `request_pass`.
    diag_generation: u64,
    /// Paths with a scheduled-but-unpublished diagnostics request; the task
    /// holding the latest generation drains the whole set.
    pending_diag_paths: HashSet<String>,
    /// Actual URIs whose last publication carried diagnostics, keyed by
    /// display path. A URI that stops receiving publications is cleared
    /// with an empty one so stale squiggles cannot linger.
    published_uris: HashMap<String, HashSet<Url>>,
    /// Bumped by every index writer; a writer installs only while the
    /// generation it captured still holds.
    index_generation: u64,
    reconciliation: Reconciliation,
    /// Files opened during initialization wait for the first workspace context.
    initial_index_pending: bool,
    /// Counts every parse performed by `parsed_file` (cache misses only).
    #[cfg(test)]
    pub(super) parse_count: Arc<std::sync::atomic::AtomicUsize>,

    /// Analysis context for files no workspace folder owns. Its `root` is the
    /// client's `rootUri` (empty without one); it is never indexed.
    fallback: FolderAnalysisContext,
    /// The full settings envelope received at initialize; dynamically
    /// added folders take their settings from it.
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

    /// Per-root analysis contexts, ordered by root path length descending
    /// for longest-prefix ownership.
    folder_contexts: Vec<FolderAnalysisContext>,
    /// Notices already sent for the currently installed serialized scopes.
    /// A repaired scope leaves this set on the next context install, so a
    /// later failure can be reported again without repeating unchanged ones.
    notified_degraded_scopes: std::collections::BTreeSet<(PathBuf, ry_workspace::InventoryFailure)>,
    /// On-disk sources discovered by the background indexer, keyed by
    /// absolute path. Open documents shadow these.
    disk_files: HashMap<String, Arc<SourceFile>>,
    /// Version-stamped tree-sitter trees for incremental parsing, served
    /// only while their version matches the current document version.
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
    /// `None` when defaults are in use. Config-relative `exclude` patterns,
    /// `include-build-ignored`, and baseline keys anchor here, as in the CLI.
    pub config_root: Option<PathBuf>,
    /// Effective `ry.toml` config: loaded from directory discovery or
    /// the editor `configuration` override resolved relative to `root`.
    pub config: ry_config::Config,
    /// Editor-supplied per-folder settings.
    pub folder_settings: FolderSettings,
    /// Local typeshed stubs loaded from this folder's `ry.toml`.
    pub stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Per-package resolution contexts keyed by the package group (see
    /// `ry_workspace::group_by_package_root`): each package is its own
    /// library scope.
    pub workspace_contexts: HashMap<Option<PathBuf>, ry_workspace::WorkspaceContext>,
    /// The baseline, loaded with the context so publication reads no disk.
    pub baseline: Option<ry_config::Baseline>,
    /// Severity filter, per-path rule policy, confidence threshold, and
    /// excludes compiled from `config` and `folder_settings`.
    pub filter: ry_checker::SeverityFilter,
    pub scoped_policy: ry_checker::ScopedRulePolicy,
    pub min_confidence: Option<ry_checker::Confidence>,
    pub excludes: ry_config::Excludes,
    /// One project cache per package group, so packages under one folder
    /// never share top-level bindings. Carried across context rebuilds.
    pub package_caches: HashMap<Option<PathBuf>, Arc<Mutex<ProjectCache>>>,
}

impl FolderAnalysisContext {
    /// Load `root`'s config, stubs, and baseline. With a `previous` context,
    /// a failed reload keeps that context's last valid config and baseline,
    /// and a stub reload where every configured directory failed keeps its
    /// stubs; deliberate removals always take effect. Disk I/O happens here;
    /// callers must run it outside the state lock.
    pub(super) fn load(
        root: PathBuf,
        folder_settings: FolderSettings,
        previous: Option<&FolderAnalysisContext>,
    ) -> Self {
        // Without a root there is nothing to discover from.
        let found = if root.as_os_str().is_empty() {
            Ok((None, ry_config::Config::default()))
        } else {
            discover_folder_config(&folder_settings, &root)
        };
        let (config_root, config) = match found {
            Ok(found) => found,
            Err(error) => {
                tracing::warn!(root = %root.display(), %error, "failed to load folder config");
                previous.map_or((None, ry_config::Config::default()), |old| {
                    (old.config_root.clone(), old.config.clone())
                })
            }
        };
        let loaded = ry_workspace::load_stub_dirs(&config.typeshed);
        for warning in &loaded.warnings {
            tracing::warn!(%warning, "user stub load problem");
        }
        let stubs = match previous {
            Some(old) if loaded.all_failed => {
                tracing::warn!(root = %root.display(), "every configured typeshed directory failed to load; retaining previous stubs");
                Arc::clone(&old.stubs)
            }
            _ => Arc::new(loaded.stubs),
        };
        let baseline_root = Some(root.as_path()).filter(|root| !root.as_os_str().is_empty());
        let baseline = match load_folder_baseline(&folder_settings, &config, baseline_root) {
            Ok(baseline) => baseline,
            Err(error) => {
                tracing::warn!(root = %root.display(), %error, "failed to load baseline");
                // Baseline keys are relative to the config anchor they were
                // loaded under, so they survive only an unchanged anchor.
                previous
                    .filter(|old| old.config_root == config_root)
                    .and_then(|old| old.baseline.clone())
            }
        };
        let mut ctx = FolderAnalysisContext {
            root,
            config_root,
            config,
            folder_settings,
            stubs,
            baseline,
            workspace_contexts: previous
                .map(|old| old.workspace_contexts.clone())
                .unwrap_or_default(),
            package_caches: previous
                .map(|old| old.package_caches.clone())
                .unwrap_or_default(),
            ..Default::default()
        };
        ctx.refresh_policy();
        ctx
    }

    /// Recompile the filter, rule policy, threshold, and excludes from the
    /// installed config and editor settings.
    fn refresh_policy(&mut self) {
        let lint = &self.folder_settings.lint;
        self.filter = ry_checker::filter_from_config(&lint.apply(&self.config));
        let protected: Vec<String> = [&lint.error, &lint.warn, &lint.ignore]
            .into_iter()
            .flatten()
            .flatten()
            .cloned()
            .collect();
        self.scoped_policy = ry_checker::ScopedRulePolicy::new(
            &self.config,
            self.config_root.as_deref(),
            &protected,
        );
        self.min_confidence = self.folder_settings.min_confidence;
        self.excludes = ry_config::Excludes::from_config(&self.config);
    }

    /// The directory config-relative excludes and baseline keys anchor at:
    /// the config's own directory, else the root.
    fn anchor(&self) -> Option<&Path> {
        self.config_root
            .as_deref()
            .or_else(|| Some(self.root.as_path()).filter(|root| !root.as_os_str().is_empty()))
    }

    fn excludes_path(&self, path: &str) -> bool {
        !self.excludes.is_empty()
            && self
                .excludes
                .matches(&ry_config::diagnostic_path(path, self.anchor()))
    }

    /// Post-process through the shared pipeline so the editor sees exactly
    /// what `ry check` reports: suppressions, severity filter, path-based
    /// confidence demotion, baseline, then the confidence threshold. Scoped
    /// rule severities match `policy_path`; without one, the base filter
    /// applies.
    fn publish(
        &self,
        diagnostics: Vec<ry_checker::Diagnostic>,
        path: &str,
        policy_path: Option<&Path>,
        file: &SourceFile,
        origin: Option<serde_json::Value>,
    ) -> Vec<LspDiagnostic> {
        let filter = policy_path.map(|native| self.scoped_policy.filter_for(native, &self.filter));
        let post = ry_checker::PostProcess {
            filter: filter.as_deref().unwrap_or(&self.filter),
            baseline: self.baseline.as_ref(),
            min_confidence: self.min_confidence.unwrap_or(ry_checker::Confidence::Low),
            repo_root: self.anchor(),
        };
        let mut diagnostics = post.pre_demotion(diagnostics, path, Some(file));
        post.demote_non_source_paths(&mut diagnostics);
        post.post_demotion(&mut diagnostics);
        diagnostics
            .iter()
            .map(|diagnostic| {
                let mut diagnostic = diagnostic_to_lsp_with_source(diagnostic, &file.source);
                diagnostic.data = origin.clone();
                diagnostic
            })
            .collect()
    }
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

/// One file fed into a package check: its path, the open document's
/// version (zero for indexed disk files), and the parsed snapshot.
type PackageFile = (String, i32, Arc<SourceFile>);

impl ProjectCache {
    #[cfg(test)]
    pub(super) fn check(
        &mut self,
        files: Vec<(String, i32, Arc<SourceFile>)>,
        user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    ) -> Vec<(String, Vec<ry_checker::Diagnostic>)> {
        self.check_with_workspace(
            files,
            user_stubs,
            ry_workspace::WorkspaceContext::default(),
            Vec::new(),
            &HashSet::new(),
        )
        .diagnostics
    }

    pub(super) fn check_with_workspace(
        &mut self,
        files: Vec<(String, i32, Arc<SourceFile>)>,
        user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
        workspace: ry_workspace::WorkspaceContext,
        records: Vec<ry_core::declarations::DeclarationRecord>,
        open_paths: &HashSet<String>,
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
        self.project.set_declaration_records(records);
        self.project.set_open_buffer_paths(open_paths.clone());
        ry_checker::frontend::set_workspace_context(&mut self.project, workspace);
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
        // `update_file` appends re-added paths at the end, so insertion
        // history would decide which same-named definition wins; the
        // incoming sequence is the canonical one.
        self.project.reorder_files(&order);
        let mut diagnostics = self.project.check_incremental();
        ry_checker::append_declaration_diagnostics(
            &mut diagnostics,
            self.project.declaration_findings(),
        );
        ProjectCheckResult {
            diagnostics,
            files: checked_files,
        }
    }
}

impl State {
    /// Replace the display-keyed source and invalidate its cached parse
    /// and hints; the next read repopulates them.
    fn update_doc(&mut self, path: &str, text: String, version: i32) {
        self.docs.insert(path.to_string(), text);
        self.versions.insert(path.to_string(), version);
        self.parsed.remove(path);
        self.hints.remove(path);
    }

    /// Apply a whole `didChange` batch for one original URI under one lock,
    /// so `didOpen`/`didClose` cannot change display-key ownership midway.
    /// Returns whether any change was applied.
    fn apply_document_changes(
        &mut self,
        path: &str,
        uri: &Url,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: i32,
    ) -> bool {
        let Some(open) = self
            .open_source_paths
            .get(path)
            .filter(|open| open.contains_key(uri))
        else {
            tracing::warn!(%uri, "ignoring change for a closed URI");
            return false;
        };
        // A colliding URI edits its own snapshot: the display-keyed text and
        // incremental tree may describe another native source.
        let colliding = open.len() > 1;
        let (mut text, mut tree) = if colliding {
            let Some((text, _)) = open[uri].shadow.clone() else {
                tracing::warn!(%uri, "missing colliding URI buffer snapshot");
                return false;
            };
            (Some(text), None)
        } else {
            (self.docs.get(path).cloned(), self.tree_for(path))
        };

        // An invalid UTF-16 range stops the batch. Prior valid edits remain
        // committed; later ranges refer to text we could not produce.
        let mut applied = false;
        for change in changes {
            match (change.range, text.as_mut()) {
                (Some(range), Some(current)) => {
                    let Some((start, end)) = range_byte_span(current, range) else {
                        tracing::error!(
                            ?range,
                            "invalid UTF-16 range in document change; aborting remaining changes in didChange batch for {path}; server and client text will desynchronize until a full sync is received"
                        );
                        break;
                    };
                    if let Some(tree) = &mut tree {
                        tree.edit(&build_input_edit_from_span(
                            current,
                            start,
                            end,
                            &change.text,
                        ));
                    }
                    current.replace_range(start..end, &change.text);
                }
                // Full replacement, or a ranged edit without old text: the
                // protocol's fallback replaces the whole document.
                _ => {
                    text = Some(change.text);
                    tree = None;
                }
            }
            applied = true;
        }
        // An empty or immediately rejected batch changes nothing, including
        // which colliding URI is active.
        let Some(text) = text.filter(|_| applied) else {
            return false;
        };
        if colliding {
            self.active_collision_uri
                .insert(path.to_string(), uri.clone());
            if let Some(source) = self
                .open_source_paths
                .get_mut(path)
                .and_then(|open| open.get_mut(uri))
            {
                source.shadow = Some((text.clone(), version));
            }
        }
        self.update_doc(path, text, version);
        match tree {
            Some(tree) => self.store_tree(path, version, tree),
            None => {
                self.trees.remove(path);
            }
        }
        true
    }

    fn has_collision(&self, path: &str) -> bool {
        self.open_source_paths
            .get(path)
            .is_some_and(|open| open.len() > 1)
    }

    /// Remove one original URI from a display-keyed document. If another
    /// URI remains, restore its own buffer when the closing URI was active
    /// and retain its native provenance for the next publication.
    fn close_open_source(&mut self, path: &str, uri: &Url) -> Option<Url> {
        let open = self.open_source_paths.get_mut(path)?;
        open.remove(uri);
        if open.is_empty() {
            self.open_source_paths.remove(path);
            self.active_collision_uri.remove(path);
            return None;
        }
        let one_left = open.len() == 1;
        let closing_active = self.active_collision_uri.get(path) == Some(uri);
        let (survivor, shadow) = open
            .iter()
            .next()
            .map(|(uri, source)| (uri.clone(), source.shadow.clone()))?;
        if one_left {
            open.get_mut(&survivor).expect("surviving source").shadow = None;
            self.active_collision_uri.remove(path);
            // Parses made during the collision carry no native identity.
            self.parsed.remove(path);
            self.hints.remove(path);
        } else if closing_active {
            self.active_collision_uri
                .insert(path.to_string(), survivor.clone());
        }
        if closing_active && let Some((text, version)) = shadow {
            self.update_doc(path, text, version);
            self.trees.remove(path);
        }
        Some(survivor)
    }

    fn newly_degraded_scopes(&mut self) -> Vec<(PathBuf, ry_workspace::InventoryFailure)> {
        let current: std::collections::BTreeSet<_> = self
            .folder_contexts
            .iter()
            .flat_map(|folder| folder.workspace_contexts.values())
            .flat_map(|context| context.degraded_scopes.iter().cloned())
            .collect();
        let new = current
            .difference(&self.notified_degraded_scopes)
            .cloned()
            .collect();
        self.notified_degraded_scopes = current;
        new
    }

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
    /// cache-invalidation half of `State::update_doc`. Test-only;
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

    /// Mutable access to the fallback editor settings for tests.
    #[cfg(test)]
    pub(super) fn folder_settings_mut(&mut self) -> &mut crate::settings::FolderSettings {
        &mut self.fallback.folder_settings
    }

    /// Mutable access to the fallback config for tests.
    #[cfg(test)]
    pub(super) fn file_config_mut(&mut self) -> &mut ry_config::Config {
        &mut self.fallback.config
    }

    /// The fallback's severity filter compiled from its current settings.
    #[cfg(test)]
    pub(super) fn effective_filter(&self) -> ry_checker::SeverityFilter {
        let mut fallback = self.fallback.clone();
        fallback.refresh_policy();
        fallback.filter
    }

    /// Find the owning [`FolderAnalysisContext`] for a document path
    /// using longest-prefix matching against folder context roots.
    pub(super) fn folder_context_for_path(&self, doc_path: &str) -> Option<&FolderAnalysisContext> {
        let path = std::path::Path::new(doc_path);
        self.folder_contexts
            .iter()
            .find(|ctx| path.starts_with(&ctx.root))
    }

    /// The owning folder's context, else the fallback context.
    pub(super) fn context_for_path(&self, doc_path: &str) -> &FolderAnalysisContext {
        self.folder_context_for_path(doc_path)
            .unwrap_or(&self.fallback)
    }

    /// Whether the owning context disabled analysis of `doc_path`: its
    /// folder is set to `enable: false`, or it is a report and reports are
    /// not opted in.
    pub(super) fn analysis_disabled(&self, doc_path: &str) -> bool {
        let ctx = self.context_for_path(doc_path);
        ctx.folder_settings.enable == Some(false)
            || !ry_workspace::DiscoveryLimits::from_config(&ctx.config)
                .admits_report(Path::new(doc_path))
    }

    /// Whether an indexed `disk_files` entry consumes `budget_root`'s
    /// `index.max-files` budget: entries attribute to their innermost
    /// containing root, as the walk caps each root's scan independently.
    fn entry_consumes_root_budget(&self, existing: &std::path::Path, budget_root: &Path) -> bool {
        existing.starts_with(budget_root)
            && !self.folder_contexts.iter().any(|ctx| {
                ctx.root.as_path() != budget_root
                    && ctx.root.starts_with(budget_root)
                    && existing.starts_with(&ctx.root)
            })
    }

    /// Whether the server should analyze and publish diagnostics for
    /// `doc_path`: not when its context disabled it; otherwise eligibility
    /// follows the owning folder's discovery rules (excludes and the size
    /// and depth caps), so an opened buffer cannot leak definitions the
    /// closed index omitted into project-wide state. A path outside every
    /// folder is judged against the fallback root when it lies under it.
    ///
    /// The size gate measures the open buffer's text length, never
    /// `fs::metadata`: unsaved content can cross the boundary without
    /// touching disk. Closed paths entered through the capped walker, so
    /// they skip the size gate.
    fn eligibility_for_path(&self, doc_path: &str) -> bool {
        let content_len = self.docs.get(doc_path).map(|text| text.len() as u64);
        self.eligibility_for_path_with_len(doc_path, content_len)
    }

    fn eligibility_for_path_with_len(&self, doc_path: &str, content_len: Option<u64>) -> bool {
        if self.analysis_disabled(doc_path) {
            return false;
        }
        let path = Path::new(doc_path);
        let ctx = self.context_for_path(doc_path);
        if ctx.root.as_os_str().is_empty() || !path.starts_with(&ctx.root) {
            return true;
        }
        ry_workspace::is_file_eligible_with_limits(
            path,
            &ctx.root,
            ctx.anchor(),
            &ctx.excludes,
            &ry_workspace::DiscoveryLimits::from_config(&ctx.config),
            content_len,
        )
    }

    /// A collision's display key remains live while any original buffer is
    /// eligible. Each shadow has its own size; the active `docs` text must
    /// never grant eligibility to an oversized sibling (or hide a small one).
    fn any_open_source_eligible(&self, doc_path: &str) -> bool {
        match self.open_source_paths.get(doc_path) {
            Some(open) if open.len() > 1 => open.values().any(|source| {
                source.shadow.as_ref().is_some_and(|(text, _)| {
                    self.eligibility_for_path_with_len(doc_path, Some(text.len() as u64))
                })
            }),
            _ => self.eligibility_for_path(doc_path),
        }
    }
}

#[cfg(test)]
mod colliding_change_tests {
    use super::*;

    #[test]
    fn ordinary_batch_keeps_incremental_tree_and_stops_at_invalid_utf16() {
        let uri = Url::parse("file:///tmp/ordinary.R").unwrap();
        let path = uri_to_path(&uri);
        let original = "x <- 1L\ny <- 2L\n";
        let (_, tree) = RParser::new()
            .unwrap()
            .parse_with_tree(&path, original, None)
            .unwrap();
        let mut state = State::default();
        state.docs.insert(path.clone(), original.to_string());
        state.versions.insert(path.clone(), 1);
        state.trees.insert(path.clone(), (1, tree));
        state.open_source_paths.insert(
            path.clone(),
            HashMap::from([(
                uri.clone(),
                OpenSource {
                    native: uri.to_file_path().ok(),
                    shadow: None,
                },
            )]),
        );
        let edit = |line, start, end, text: &str| TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(line, start),
                end: Position::new(line, end),
            }),
            range_length: None,
            text: text.to_string(),
        };
        let changes = vec![edit(0, 5, 7, "\"😀\""), edit(1, 5, 7, "3L")];
        assert!(state.apply_document_changes(&path, &uri, changes, 2));
        let expected = "x <- \"😀\"\ny <- 3L\n";
        assert_eq!(state.docs[&path], expected);
        assert_eq!(state.versions[&path], 2);
        let edited_tree = state.tree_for(&path).expect("ranged edits retain the tree");
        let (_, incremental) = RParser::new()
            .unwrap()
            .parse_with_tree(&path, expected, Some(&edited_tree))
            .unwrap();
        let (_, cold) = RParser::new()
            .unwrap()
            .parse_with_tree(&path, expected, None)
            .unwrap();
        assert_eq!(
            incremental.root_node().to_sexp(),
            cold.root_node().to_sexp()
        );

        // UTF-16 column 7 lands inside the astral character. The following
        // full replacement must not run after this malformed ranged edit.
        let invalid = vec![
            edit(0, 7, 7, "bad"),
            TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "stale <- TRUE\n".to_string(),
            },
        ];
        assert!(!state.apply_document_changes(&path, &uri, invalid, 3));
        assert_eq!(state.docs[&path], expected);
        assert_eq!(state.versions[&path], 2);
        assert!(state.tree_for(&path).is_some());
    }

    #[tokio::test]
    async fn simultaneous_ranged_edits_keep_each_native_snapshot() {
        let first = Url::parse("file:///tmp/bad%FF.R").unwrap();
        let second = Url::parse("file:///tmp/bad%FE.R").unwrap();
        let path = uri_to_path(&first);
        assert_eq!(path, uri_to_path(&second));
        let first_text = "first🙂\n#| x integer\n";
        let second_text = "g\n# ordinary\n";
        let mut state = State::default();
        state.docs.insert(path.clone(), second_text.to_string());
        state.versions.insert(path.clone(), 1);
        state
            .active_collision_uri
            .insert(path.clone(), second.clone());
        state.open_source_paths.insert(
            path.clone(),
            HashMap::from([
                (
                    first.clone(),
                    OpenSource {
                        native: first.to_file_path().ok(),
                        shadow: Some((first_text.to_string(), 1)),
                    },
                ),
                (
                    second.clone(),
                    OpenSource {
                        native: second.to_file_path().ok(),
                        shadow: Some((second_text.to_string(), 1)),
                    },
                ),
            ]),
        );
        let shared = Arc::new(Mutex::new(state));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let edit = |uri: Url, end: u32, replacement: &'static str| {
            let shared = Arc::clone(&shared);
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                let changes = vec![TextDocumentContentChangeEvent {
                    range: Some(Range {
                        start: Position::new(0, 0),
                        end: Position::new(0, end),
                    }),
                    range_length: None,
                    text: replacement.to_string(),
                }];
                let applied = shared
                    .lock()
                    .await
                    .apply_document_changes(&path, &uri, changes, 7);
                assert!(applied);
            })
        };
        let (a, b) = tokio::join!(
            edit(first.clone(), 7, "one"),
            edit(second.clone(), 1, "two")
        );
        a.unwrap();
        b.unwrap();
        let state = shared.lock().await;
        let open = &state.open_source_paths[&path];
        assert_eq!(
            open[&first].shadow.as_ref().unwrap(),
            &("one\n#| x integer\n".to_string(), 7)
        );
        assert_eq!(
            open[&second].shadow.as_ref().unwrap(),
            &("two\n# ordinary\n".to_string(), 7)
        );
        let active = &state.active_collision_uri[&path];
        assert_eq!(&state.docs[&path], &open[active].shadow.as_ref().unwrap().0);
    }
}

/// How a background index pass ended. `Installed` is the only outcome
/// that put new bytes in `disk_files`; `SettledWithoutInstall` cleared
/// `initial_index_pending` without touching the map (no roots, or a failed
/// walk); `Superseded` lost the generation race to a newer writer.
enum BackgroundIndexOutcome {
    Installed,
    SettledWithoutInstall,
    Superseded,
}

/// Spawn one reconciliation driver task. A plain function boundary keeps
/// the driver's future out of the wake site's future; an inline spawn
/// would make the Send obligations of the two futures circular.
fn spawn_reconciliation_driver(backend: Backend) {
    tokio::spawn(async move {
        backend.run_reconciliation().await;
    });
}

impl Backend {
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
            // Sources, so edits to unopened files refresh the disk index;
            // per-file walk rules apply when the event is processed.
            serde_json::json!({"globPattern": format!(
                "**/*.{{{}}}",
                ry_workspace::source_extension_globs().collect::<Vec<_>>().join(",")
            )}),
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
            let (text, version, old_tree, native) = {
                let state = self.state.lock().await;
                let (Some(text), Some(version)) = (state.docs.get(path), state.versions.get(path))
                else {
                    return None;
                };
                if let Some(file) = state.cached_parse(path) {
                    return Some((file, text.clone()));
                }
                // A colliding display key has no single native identity.
                let native = state
                    .open_source_paths
                    .get(path)
                    .filter(|open| open.len() == 1)
                    .and_then(|open| open.values().next()?.native.clone());
                (text.clone(), *version, state.tree_for(path), native)
            };
            // A test can park here to land a newer edit before this parse
            // installs; the version checks below then reject it.
            #[cfg(feature = "test-util")]
            let paused_parse = crate::test_seam::pause(Seam::Parse).await;
            let mut parser = RParser::new().ok()?;
            let (mut parsed, new_tree) = ry_workspace::reports::parse_source_with_tree(
                &mut parser,
                path,
                &text,
                old_tree.as_ref(),
            )
            .ok()?;
            parsed.native_path = native;
            let file = Arc::new(parsed);
            let mut state = self.state.lock().await;
            // A different original URI can replace this display-keyed source
            // with the same numeric version. The parsed text and version must
            // still describe the current snapshot before either cache entry
            // is installed; a version-only check can publish the first URI's
            // AST through the second URI's buffer.
            let stored = if state.versions.get(path).copied() == Some(version)
                && state.docs.get(path) == Some(&text)
            {
                if let Some(tree) = new_tree {
                    state.store_tree(path, version, tree);
                }
                state.record_parse(path, version, Arc::clone(&file))
            } else {
                false
            };
            #[cfg(feature = "test-util")]
            if paused_parse {
                crate::test_seam::landed(Seam::Parse);
            }
            if stored {
                return Some((file, text));
            }
        }
    }

    /// Return hints from one current parse and its assignment-site inference.
    async fn hints_for(&self, path: &str) -> Option<Vec<InlayHint>> {
        let user_stubs = {
            let state = self.state.lock().await;
            let stubs = &state.context_for_path(path).stubs;
            if let Some(version) = state.versions.get(path).copied()
                && let Some(cached) = state.hints.get(path)
                && cached.version == version
                && Arc::ptr_eq(&cached.stubs, stubs)
            {
                return Some(cached.hints.clone());
            }
            Arc::clone(stubs)
        };
        let (file, text) = self.parsed_file(path).await?;
        let mut checker = ry_checker::Checker::new(path);
        checker.set_user_stubs(Arc::clone(&user_stubs));
        checker.enable_assignment_capture();
        checker.check(&file);
        let hints = collect_inlay_hints(&checker.take_assignment_types(), &text);
        let mut state = self.state.lock().await;
        if !Arc::ptr_eq(&state.context_for_path(path).stubs, &user_stubs) {
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
    /// one item per folder root plus a final item for the fallback context.
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
                    scope_uri: Url::from_file_path(&state.fallback.root).ok(),
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
        let state = &mut *state;
        let contexts = state
            .folder_contexts
            .iter_mut()
            .chain(std::iter::once(&mut state.fallback));
        for (ctx, value) in contexts.zip(&values) {
            if let Ok(settings) = serde_json::from_value::<FolderSettings>(value.clone()) {
                ctx.folder_settings = settings;
            }
            ctx.refresh_policy();
        }
        if let Some(value) = values.get(state.folder_contexts.len())
            && let Ok(settings) = serde_json::from_value::<FolderSettings>(value.clone())
        {
            state.server_settings.global_settings = settings;
        }
    }

    /// Incrementally update the project and publish diagnostics for every
    /// checked file: an edit to a definition can change diagnostics in its
    /// cross-file callers.
    ///
    /// `requested` is the set of paths the debounce drained for this
    /// generation. A requested path that is no longer eligible still gets
    /// an immediate empty publication (the didOpen acknowledgment for a
    /// document opened into a disabled folder, an excluded path, or over
    /// a size/depth cap); every other URI that drops out of the check is
    /// reconciled at the end of the pass by
    /// [`Backend::clear_dropped_diagnostics`].
    async fn publish_diagnostics(&self, requested: HashSet<String>, generation: u64) {
        // Snapshot under the lock, then check without it so a slow check
        // does not block other requests.
        let (
            doc_versions,
            requested_ineligible,
            supports_diagnostic_data,
            open_source_paths,
            active_collision_uris,
            eligible_collision_sources,
        ) = {
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
                    .filter(|p| !state.any_open_source_eligible(p))
                    .cloned()
                    .collect::<Vec<_>>(),
                state.supports_diagnostic_data,
                state.open_source_paths.clone(),
                state.active_collision_uri.clone(),
                state
                    .open_source_paths
                    .iter()
                    .filter(|(_, open)| open.len() > 1)
                    .map(|(path, open)| {
                        let eligible: HashMap<Url, (String, i32)> = open
                            .iter()
                            .filter_map(|(uri, source)| {
                                let (text, version) = source.shadow.as_ref()?;
                                state
                                    .eligibility_for_path_with_len(path, Some(text.len() as u64))
                                    .then(|| (uri.clone(), (text.clone(), *version)))
                            })
                            .collect();
                        (path.clone(), state.context_for_path(path).clone(), eligible)
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let publication_uris = |path: &str| source_uris(&open_source_paths, path);
        let colliding = |path: &str| {
            open_source_paths
                .get(path)
                .is_some_and(|open| open.len() > 1)
        };
        if !requested_ineligible.is_empty() {
            let uris_to_clear = {
                let mut state = self.state.lock().await;
                let mut uris_to_clear = HashSet::new();
                for path in &requested_ineligible {
                    uris_to_clear.extend(publication_uris(path));
                    uris_to_clear.extend(state.published_uris.remove(path).unwrap_or_default());
                }
                uris_to_clear
            };
            for uri in uris_to_clear {
                self.client.publish_diagnostics(uri, Vec::new(), None).await;
            }
        }

        let mut open_files = Vec::with_capacity(doc_versions.len());
        for (doc_path, version) in &doc_versions {
            let Some((file, _)) = self.parsed_file(doc_path).await else {
                continue;
            };
            open_files.push((doc_path.clone(), *version, file));
        }
        // One path-keyed source view: an open buffer owns its path, eligible
        // or not, so its disk twin never contributes, and an ineligible
        // buffer contributes nothing. Sorting the union by path gives the
        // CLI's canonical order, which decides same-name shadowing; each
        // owner's files keep that order through the grouping below.
        let mut project_files: Vec<(Option<PathBuf>, PackageFile)> = {
            let state = self.state.lock().await;
            let disk = state
                .disk_files
                .iter()
                .filter(|(p, _)| !state.docs.contains_key(p.as_str()))
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

        // A display path can stand for several native URIs. Keep each
        // admitted buffer's annotation status, comments, and version with
        // that URI; the active sibling cannot supply any of those values.
        let mut collision_diagnostics: HashMap<String, HashMap<Url, Vec<LspDiagnostic>>> =
            HashMap::new();
        for (path, ctx, sources) in eligible_collision_sources {
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
                    );
                    by_uri.insert(uri, warnings);
                }
            }
            collision_diagnostics.insert(path, by_uri);
        }

        // Partition by owning context, then by package group: each R package
        // is a separate library scope and checks through its own cache.
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
                // The owning folder was removed mid-pass; its removal already
                // cleared these URIs.
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

        // Every open buffer, including ineligible ones and those owned by
        // another partition, hides its possibly stale disk copy from box.
        let open_paths: HashSet<String> = open_source_paths.keys().cloned().collect();
        let mut all_results: Vec<(Arc<FolderAnalysisContext>, ProjectCheckResult)> = Vec::new();
        for job in jobs {
            // Colliding snapshots are handled by the per-URI pass above.
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

        // An edit that arrived while parsing/checking invalidates this whole
        // project result because every open document is republished below.
        if self.state.lock().await.diag_generation != generation {
            return;
        }

        let diagnostic_versions: HashMap<_, _> = doc_versions.into_iter().collect();

        // Publish each file through the context it was checked under.
        // `published` records what this pass sent for the reconciliation below.
        let mut published: Vec<(String, HashSet<Url>, HashSet<Url>)> = Vec::new();
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
                let diagnostics = ctx.publish(
                    diagnostics,
                    &diagnostic_path,
                    Some(Path::new(&diagnostic_path)),
                    file,
                    diagnostic_versions
                        .get(&diagnostic_path)
                        .filter(|_| supports_diagnostic_data)
                        .map(|version| diagnostic_origin(&diagnostic_path, *version, generation)),
                );
                let uris = publication_uris(&diagnostic_path);
                if uris.len() == 1 {
                    let uri = uris[0].clone();
                    let non_empty = if diagnostics.is_empty() {
                        HashSet::new()
                    } else {
                        HashSet::from([uri.clone()])
                    };
                    self.client
                        .publish_diagnostics(
                            uri,
                            diagnostics,
                            diagnostic_versions.get(&diagnostic_path).copied(),
                        )
                        .await;
                    published.push((diagnostic_path, uris.into_iter().collect(), non_empty));
                    continue;
                }
                let mut warnings = collision_diagnostics
                    .remove(&diagnostic_path)
                    .unwrap_or_default();
                let mut non_empty = HashSet::new();
                for uri in &uris {
                    // The active display-keyed buffer owns ordinary check
                    // results. Other native URIs only receive the shared
                    // ambiguity status, never another buffer's findings.
                    let mut for_uri = if active_collision_uris.get(&diagnostic_path) == Some(uri) {
                        diagnostics.clone()
                    } else {
                        Vec::new()
                    };
                    for_uri.extend(warnings.remove(uri).unwrap_or_default());
                    if !for_uri.is_empty() {
                        non_empty.insert(uri.clone());
                    }
                    self.client
                        .publish_diagnostics(
                            uri.clone(),
                            for_uri,
                            (active_collision_uris.get(&diagnostic_path) == Some(uri))
                                .then(|| diagnostic_versions.get(&diagnostic_path).copied())
                                .flatten(),
                        )
                        .await;
                }
                published.push((diagnostic_path, uris.into_iter().collect(), non_empty));
            }
        }

        // If the active colliding buffer is over cap, no project file was
        // checked for this display key. Eligible inactive buffers still own
        // their annotation warning, so publish them without admitting the
        // oversized active text into inference or reparsing it.
        for (path, mut warnings) in collision_diagnostics {
            let uris = publication_uris(&path);
            let mut non_empty = HashSet::new();
            for uri in &uris {
                let for_uri = warnings.remove(uri).unwrap_or_default();
                if !for_uri.is_empty() {
                    non_empty.insert(uri.clone());
                }
                self.client
                    .publish_diagnostics(uri.clone(), for_uri, None)
                    .await;
            }
            published.push((path, uris.into_iter().collect(), non_empty));
        }

        // A URI this pass no longer publishes to keeps its old squiggles
        // unless it is explicitly cleared.
        let previous_uris = {
            let mut state = self.state.lock().await;
            let mut previous_uris = Vec::new();
            for (path, destinations, non_empty) in published {
                let previous = state.published_uris.remove(&path).unwrap_or_default();
                previous_uris.extend(previous.difference(&destinations).cloned());
                if !non_empty.is_empty() {
                    state.published_uris.insert(path, non_empty);
                }
            }
            previous_uris
        };
        for uri in previous_uris {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
        self.clear_dropped_diagnostics().await;
    }

    /// Publish empty diagnostics for every tracked URI that left the
    /// analysis set since its last publication: the path is no longer
    /// eligible, or it is neither open nor in the disk index. Deciding
    /// against current state, not one pass's snapshot, also heals a slower
    /// older pass that publishes after a newer one cleared a URI.
    async fn clear_dropped_diagnostics(&self) {
        let dropped: Vec<Url> = {
            let mut state = self.state.lock().await;
            let dropped: Vec<String> = state
                .published_uris
                .keys()
                .filter(|path| {
                    !state.any_open_source_eligible(path.as_str())
                        || (!state.docs.contains_key(path.as_str())
                            && !state.disk_files.contains_key(path.as_str()))
                })
                .cloned()
                .collect();
            dropped
                .iter()
                .flat_map(|path| state.published_uris.remove(path).unwrap_or_default())
                .collect()
        };
        for uri in dropped {
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
        }
    }

    /// Discover and parse every source under the workspace folders and
    /// install the result in `disk_files`. Never publishes: callers
    /// republish afterwards. Returns whether this pass settled the index
    /// duty it owed (installed, or settled without installing while its
    /// generation held); false when a newer writer superseded it. Callers
    /// that need to know whether bytes landed use
    /// [`Backend::background_index_outcome`].
    async fn reindex(&self) -> bool {
        matches!(
            self.background_index_outcome().await,
            BackgroundIndexOutcome::Installed | BackgroundIndexOutcome::SettledWithoutInstall
        )
    }

    async fn background_index_outcome(&self) -> BackgroundIndexOutcome {
        #[cfg(feature = "test-util")]
        crate::test_seam::bump(Counter::BackgroundIndexSpawns);
        // Only workspace folders are indexed: the fallback root never is,
        // so removing every folder stops indexing its directory.
        let (roots, index_gen) = {
            let mut state = self.state.lock().await;
            state.index_generation = state.index_generation.wrapping_add(1);
            let roots: Vec<_> = state
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
            (roots, state.index_generation)
        };
        if roots.is_empty() {
            let mut state = self.state.lock().await;
            if state.index_generation == index_gen {
                state.initial_index_pending = false;
                return BackgroundIndexOutcome::SettledWithoutInstall;
            }
            return BackgroundIndexOutcome::Superseded;
        }

        #[cfg(feature = "test-util")]
        crate::test_seam::pause(Seam::InitialIndex).await;
        let indexed = tokio::task::spawn_blocking(move || {
            let mut all_disk_files: HashMap<String, Arc<SourceFile>> = HashMap::new();
            let mut contexts = Vec::new();
            let mut all_truncated: Vec<(PathBuf, ry_workspace::TruncationReport)> = Vec::new();
            for (root, config_root, config, stubs) in &roots {
                let outcome = crate::index::index_workspace(root, config_root.as_deref(), config);
                for report in outcome.truncated.iter().filter(|t| t.any_hit()) {
                    all_truncated.push((root.clone(), report.clone()));
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
                    match group.context {
                        Ok(context) => {
                            folder_contexts.insert(group.key, context);
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
                #[cfg(feature = "test-util")]
                crate::test_seam::pause(Seam::ScanCommit).await;
                let mut state = self.state.lock().await;
                if state.index_generation != index_gen {
                    tracing::debug!(
                        gen = index_gen,
                        current = state.index_generation,
                        "discarding stale background index results"
                    );
                    return BackgroundIndexOutcome::Superseded;
                }
                state.disk_files = disk_files;
                for ctx in &mut state.folder_contexts {
                    if let Some((_, wc)) = contexts.iter().find(|(root, _)| root == &ctx.root) {
                        ctx.workspace_contexts = wc.clone();
                    }
                }
                state.initial_index_pending = false;
                let newly_degraded = state.newly_degraded_scopes();
                drop(state);
                for (path, reason) in &newly_degraded {
                    self.client
                        .log_message(MessageType::WARNING, reason.notice(path))
                        .await;
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
                BackgroundIndexOutcome::Installed
            }
            Err(error) => {
                tracing::warn!(%error, "background workspace index failed");
                let mut state = self.state.lock().await;
                if state.index_generation == index_gen {
                    state.initial_index_pending = false;
                    BackgroundIndexOutcome::SettledWithoutInstall
                } else {
                    BackgroundIndexOutcome::Superseded
                }
            }
        }
    }

    /// Request a pass for every open document after a global state change
    /// (settings, watched files), so it takes effect without an edit.
    async fn republish_all_open_documents(&self) {
        let open: Vec<String> = self.state.lock().await.docs.keys().cloned().collect();
        self.request_pass(open).await;
    }

    /// Start at most one driver under the same lock used to enqueue work.
    async fn wake_reconciliation(&self) {
        let spawn_now = self.state.lock().await.reconciliation.start_driver();
        if !spawn_now {
            return;
        }
        #[cfg(feature = "test-util")]
        crate::test_seam::bump(Counter::ReconciliationDriverSpawns);
        spawn_reconciliation_driver(self.clone());
    }

    /// Retry bytes, context, and publication in order until retained work settles.
    async fn run_reconciliation(&self) {
        loop {
            let Some((round, retired_before)) =
                self.state.lock().await.reconciliation.begin_round()
            else {
                return;
            };
            #[cfg(feature = "test-util")]
            crate::test_seam::bump(Counter::ReconciliationRounds);
            // A pending initial index would make every refresh lose its
            // generation race to the completing pass and gate the
            // publications below; settle that duty first, exactly like
            // `refresh_package_contexts`.
            if self.state.lock().await.initial_index_pending {
                self.reindex().await;
            }
            let mut follow_up: Vec<(String, u64)> = Vec::new();
            let mut did_work = false;
            for path in round {
                let duty = self.state.lock().await.reconciliation.drivable_duty(&path);
                match duty {
                    // Retired while an earlier path was being driven
                    // (its own event's refresh completed the follow-up),
                    // owned by an in-flight refresh, or cancelled by
                    // shutdown: nothing for this round to do.
                    None => {}
                    Some((RefreshDuty::BytesContextPublication, _)) => {
                        did_work = true;
                        // The landing's own claim epoch — returned by
                        // the refresh, never looked up here — is the
                        // token the follow-up acknowledges with.
                        if let Some(epoch) = self
                            .refresh_disk_entry(std::path::PathBuf::from(&path))
                            .await
                        {
                            follow_up.push((path, epoch));
                        }
                    }
                    // Bytes already settled: re-drive only the context
                    // refresh and the publication it gates, without
                    // re-reading (a re-landing would also bump the
                    // generation and retire unrelated in-flight scans).
                    Some((RefreshDuty::ContextPublication, epoch)) => {
                        did_work = true;
                        follow_up.push((path, epoch));
                    }
                }
            }
            if !did_work {
                // Test seam: park between the round's duty checks and
                // the idle decision below, holding no lock, so a test
                // can interleave a refresh exit exactly inside the
                // window the re-check below exists for.
                #[cfg(feature = "test-util")]
                crate::test_seam::pause(Seam::DriverIdle).await;
                if self.state.lock().await.reconciliation.idle_if_blocked() {
                    return;
                }
                continue;
            }
            if !follow_up.is_empty() {
                // Shutdown may have landed while the dispatched
                // refreshes were running: their landings are already
                // committed index state, but the follow-up publications
                // belong to the ended session and must not go out. The
                // round head performs the idle transition on the next
                // pass.
                if self.state.lock().await.reconciliation.is_shutting_down() {
                    continue;
                }
                // Same order as the watched handler: advance the contexts,
                // then publish, then acknowledge.
                let ctx_settled = self.refresh_package_contexts(&follow_up).await;
                self.request_pass(follow_up.iter().map(|(path, _)| path.clone()))
                    .await;
                self.acknowledge_publications(&ctx_settled).await;
            }
            let paced = {
                let mut state = self.state.lock().await;
                match state
                    .reconciliation
                    .reconcile_round_progress(retired_before)
                {
                    // Everything is RETAINED; the driver yields for the
                    // computed backoff and retries. The retained paths
                    // ride along only on the round that ENTERS the stall
                    // episode, so the visible warning fires once per
                    // episode.
                    ReconcileRoundProgress::Pacing { delay, warn } => {
                        Some((warn.then(|| state.reconciliation.pending_paths()), delay))
                    }
                    ReconcileRoundProgress::Progress | ReconcileRoundProgress::Stalled => None,
                }
            };
            if let Some((retained, delay)) = paced {
                if let Some(retained) = retained {
                    tracing::warn!(
                        paths = ?retained,
                        rounds = RECONCILE_ROUNDS_BEFORE_PACING,
                        "watched-file reconciliation stalled; retaining the obligations and pacing retries (final analysis for these paths may lag until their workspace settles)"
                    );
                } else {
                    // The episode's entry warned; every later paced
                    // round stays observable at debug level, so an
                    // operator can still see the driver alive inside a
                    // permanently pathological episode without warn
                    // spam per retry.
                    tracing::debug!(
                        delay_ms = delay.as_millis() as u64,
                        "watched-file reconciliation still stalled; retaining the obligations and pacing the retry"
                    );
                }
                // Yield with no lock held, then retry: the pace bounds
                // rounds per wall-clock while completion stays
                // impossible, and the coalescing contract is untouched —
                // the active flag keeps every enqueue-side wake a no-op
                // for the delay's duration, and the next round
                // re-snapshots under the lock.
                tokio::time::sleep(delay).await;
            }
        }
    }

    /// Refresh one `disk_files` entry from disk under the owning folder's
    /// eligibility and the walk's per-file admission rules, through the same
    /// loader the indexer uses. A missing, inadmissible, or unreadable file,
    /// or one no folder owns, leaves the index, so a rescan cannot disagree
    /// about membership.
    ///
    /// Returns the refresh's claim epoch when it landed. The epoch was
    /// claimed before the read, so the caller's context and publication
    /// follow-up acknowledges exactly the revision it covered. A landing
    /// claims the next `index_generation` in its commit critical section,
    /// retiring in-flight scans; the caller must not bump again. `None` is
    /// terminal for this event: a newer same-path refresh, an open buffer, a
    /// cap refusal, or an unlanded backstop owns the publication.
    async fn refresh_disk_entry(&self, path: PathBuf) -> Option<u64> {
        let path_string = path.to_string_lossy().into_owned();
        // Claim once, before the first read: the claim orders same-path
        // refreshes by start and enqueues the reconciliation obligation
        // before the read can lose its races. A shutting-down session
        // claims nothing.
        let refresh_epoch = self
            .state
            .lock()
            .await
            .reconciliation
            .claim_refresh_epoch(&path_string)?;
        let landed = self.refresh_disk_entry_claimed(path, refresh_epoch).await;
        let retained_idle = {
            let mut state = self.state.lock().await;
            state.reconciliation.note_refresh_exit(&path_string);
            // Landed refreshes leave context/publication to their caller.
            matches!(
                state.reconciliation.drivable_duty(&path_string),
                Some((RefreshDuty::BytesContextPublication, _))
            )
        };
        if retained_idle {
            self.wake_reconciliation().await;
        }
        landed.then_some(refresh_epoch)
    }

    /// The read and commit under a claim `refresh_disk_entry` already took.
    /// A commit that lost a generation race retries once, then converges
    /// through a full scan.
    async fn refresh_disk_entry_claimed(&self, path: PathBuf, refresh_epoch: u64) -> bool {
        let path_string = path.to_string_lossy().into_owned();
        for attempt in 0..2 {
            // Snapshot admission and the generation the commit must still
            // hold, so the checks agree even if a reload lands mid-refresh.
            let (admission, is_open, superseded, refresh_gen) = {
                let state = self.state.lock().await;
                let admission = state
                    .folder_context_for_path(&path_string)
                    .filter(|_| state.eligibility_for_path(&path_string))
                    .map(WalkAdmission::for_folder);
                (
                    admission,
                    state.docs.contains_key(&path_string),
                    !state.reconciliation.owns_epoch(&path_string, refresh_epoch),
                    state.index_generation,
                )
            };
            if is_open {
                // The open buffer owns the path's analysis; a later didClose
                // re-reads through its own refresh.
                let mut state = self.state.lock().await;
                state
                    .reconciliation
                    .settle_pending_refresh(&path_string, refresh_epoch, false);
                return false;
            }
            // A newer same-path refresh owns the commit; skip the read it
            // would discard. The claim stays with the newer refresh.
            if superseded {
                return false;
            }
            let budget = admission
                .as_ref()
                .map(|admission| (admission.root.clone(), admission.limits.max_files));
            let read_path = path.clone();
            let parsed = tokio::task::spawn_blocking(move || {
                admission.filter(|admission| admission.admits(&read_path))?;
                ry_workspace::parse_source_file(&read_path, None)
                    .ok()
                    .map(Arc::new)
            })
            .await
            .ok()
            .flatten();
            #[cfg(feature = "test-util")]
            crate::test_seam::pause_and_land(Seam::RefreshCommit).await;
            let mut state = self.state.lock().await;
            // A didOpen landed during the read; its buffer owns the path.
            if state.docs.contains_key(&path_string) {
                state
                    .reconciliation
                    .settle_pending_refresh(&path_string, refresh_epoch, false);
                return false;
            }
            // A newer writer moved the generation during the read. It may be
            // unrelated to this path, so retry from current state rather than
            // strand the event; the re-read postdates every writer that beat
            // this attempt.
            if state.index_generation != refresh_gen {
                drop(state);
                if attempt == 0 {
                    tracing::debug!(path = %path_string, "retrying per-file disk refresh after a lost generation race");
                    continue;
                }
                tracing::warn!(
                    path = %path_string,
                    "per-file disk refresh lost two generation races; converging through a full scan"
                );
                break;
            }
            // Only the most recently started refresh for a path commits, so
            // last-write-wins follows read order, not commit order.
            if !state.reconciliation.owns_epoch(&path_string, refresh_epoch) {
                return false;
            }
            match (parsed, budget) {
                (Some(file), Some((budget_root, max_files))) => {
                    // `index.max-files` is not a path property: refuse a new
                    // entry once the owning root is at its budget, first come
                    // first served like the walk. Decided under the lock so
                    // racing refreshes cannot both grow the map.
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
                        // Terminal refusal: the path stays unindexed, as a
                        // fresh scan would leave it, and its claim retires.
                        state.reconciliation.forget_epoch(&path_string);
                        state.reconciliation.settle_pending_refresh(
                            &path_string,
                            refresh_epoch,
                            false,
                        );
                        return false;
                    }
                    // The insert, the settlement, and the generation bump
                    // share one lock hold, so no scan commits in between.
                    state
                        .reconciliation
                        .settle_pending_refresh(&path_string, refresh_epoch, true);
                    state.disk_files.insert(path_string, file);
                }
                _ => {
                    // Inadmissible or unreadable: like the walk, never indexed.
                    // The removal lands like an insert, and the claim leaves
                    // with the entry it ordered.
                    state
                        .reconciliation
                        .settle_pending_refresh(&path_string, refresh_epoch, true);
                    state.disk_files.remove(&path_string);
                    state.reconciliation.forget_epoch(&path_string);
                }
            }
            state.index_generation = state.index_generation.wrapping_add(1);
            drop(state);
            #[cfg(feature = "test-util")]
            crate::test_seam::pause_and_land(Seam::PostRefreshCommit).await;
            return true;
        }
        // Two lost races: converge through the generation-guarded full scan,
        // which re-reads every path. Only an installed scan counts as this
        // path's landing; anything else leaves the obligation retained for
        // the reconciliation driver.
        let landed = matches!(
            self.background_index_outcome().await,
            BackgroundIndexOutcome::Installed
        );
        if landed {
            let mut state = self.state.lock().await;
            state
                .reconciliation
                .settle_pending_refresh(&path_string, refresh_epoch, true);
        }
        landed
    }

    /// Retire the obligations whose context and publication settled, each
    /// against its landing's own epoch, so an older refresh completing late
    /// cannot retire a newer event's re-armed obligation. Unsettled paths
    /// stay for the reconciliation driver.
    async fn acknowledge_publications(&self, settled: &[(String, u64)]) {
        // Test seam: park between the context/publication settlement and
        // the obligation acknowledgement (see `test_seam`).
        #[cfg(feature = "test-util")]
        crate::test_seam::pause(Seam::PublicationAck).await;
        let mut state = self.state.lock().await;
        for (path, epoch) in settled {
            state
                .reconciliation
                .complete_pending_publication(path, *epoch);
        }
    }

    /// Re-resolve the owning package group's resolution context after
    /// per-file disk refreshes landed. A refreshed parse reaches
    /// `disk_files`, but its per-file resolution entries are otherwise
    /// rebuilt only by a full background scan. Each affected group re-runs
    /// the scan's `resolve_workspace_context` pass over the current disk
    /// index, so an incremental install never disagrees with a fresh scan.
    ///
    /// Installs are generation-guarded like every other index writer: a lost
    /// race retries once from a fresh snapshot, then falls back to a full
    /// background scan.
    ///
    /// Each `(path, epoch)` pair carries its landing's completion token, and
    /// the returned settled pairs keep it, so the caller acknowledges only
    /// the revision this work covered (see [`Self::acknowledge_publications`]).
    ///
    /// A landed refresh retires any stale in-flight background pass. If that
    /// was the initial index, a fresh pass takes over first; otherwise
    /// publications would stay gated for the rest of the session.
    async fn refresh_package_contexts(&self, landed: &[(String, u64)]) -> Vec<(String, u64)> {
        if self.state.lock().await.initial_index_pending {
            self.reindex().await;
        }
        // At most one group install per owning folder per package root.
        // Returns the paths whose context duty is settled — a landed
        // group install, a vanished owning folder (the removal path
        // already cleared and republished), or a root-owned partition
        // (no stored context to advance) — each with the epoch of the
        // landing that owed it. Paths whose group could not settle keep
        // their reconciliation obligation, so the driver retries the
        // group instead of leaving the landed bytes paired with a
        // stale import context forever.
        let mut groups: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
        let mut settled: Vec<(String, u64)> = Vec::new();
        let mut path_group: Vec<((String, u64), usize)> = Vec::new();
        {
            let state = self.state.lock().await;
            for (path, epoch) in landed {
                let folder_root = match state.folder_context_for_path(path) {
                    Some(ctx) => ctx.root.clone(),
                    // The fallback context stores no package contexts, so
                    // there is nothing to advance.
                    None => {
                        settled.push((path.clone(), *epoch));
                        continue;
                    }
                };
                let package_root = ry_workspace::analysis_group_key(std::path::Path::new(path));
                let index = match groups
                    .iter()
                    .position(|group| *group == (folder_root.clone(), package_root.clone()))
                {
                    Some(index) => index,
                    None => {
                        groups.push((folder_root, package_root));
                        groups.len() - 1
                    }
                };
                path_group.push(((path.clone(), *epoch), index));
            }
        }
        let mut group_settled = vec![false; groups.len()];
        for (index, (folder_root, package_root)) in groups.iter().enumerate() {
            group_settled[index] = self
                .refresh_one_package_context(folder_root, package_root)
                .await;
        }
        for ((path, epoch), index) in path_group {
            if group_settled[index] {
                settled.push((path, epoch));
            }
        }
        settled
    }

    /// Snapshot, resolve, and generation-guarded install for one package
    /// group, with one retry and a full-scan backstop on lost races (see
    /// [`Backend::refresh_package_contexts`]). Returns whether the
    /// group's context duty settled: a landed install, an Installed
    /// backstop scan (the scan resolves every group wholesale), or a
    /// vanished owning folder. A superseded or settled-without-install
    /// backstop and a failed resolve task leave the group unsettled —
    /// the reconciliation driver retries it rather than letting the
    /// landed bytes pair with a stale import context.
    async fn refresh_one_package_context(
        &self,
        folder_root: &Path,
        package_root: &Option<PathBuf>,
    ) -> bool {
        for _ in 0..2 {
            // Snapshot the folder's disk files (open buffers stay out, as in
            // the scan), config, stubs, and the generation the install must
            // still hold. Grouping probes the filesystem, so it runs in the
            // blocking task.
            struct Snapshot {
                candidates: Vec<(String, Arc<SourceFile>)>,
                config: ry_config::Config,
                stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
                generation: u64,
            }
            let snapshot = {
                let state = self.state.lock().await;
                let ctx = match state
                    .folder_contexts
                    .iter()
                    .find(|ctx| ctx.root == *folder_root)
                {
                    Some(ctx) => ctx,
                    // The folder was removed; its removal already cleared
                    // and republished.
                    None => return true,
                };
                let mut candidates: Vec<(String, Arc<SourceFile>)> = state
                    .disk_files
                    .iter()
                    .filter(|(path, _)| std::path::Path::new(path).starts_with(folder_root))
                    .map(|(path, file)| (path.clone(), Arc::clone(file)))
                    .collect();
                candidates.sort_by(|a, b| a.0.cmp(&b.0));
                Snapshot {
                    candidates,
                    config: ctx.config.clone(),
                    stubs: Arc::clone(&ctx.stubs),
                    generation: state.index_generation,
                }
            };
            let generation = snapshot.generation;
            let (key, root) = (package_root.clone(), folder_root.to_path_buf());
            let resolved = tokio::task::spawn_blocking(move || {
                // The full scan's grouping, so a report stays its own group.
                let groups = ry_workspace::group_by_package_root(
                    snapshot.candidates.iter().map(|(path, _)| path.as_str()),
                );
                let files: Vec<&SourceFile> = groups
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .map(|&index| snapshot.candidates[index].1.as_ref())
                    .collect();
                ry_workspace::resolve_group(
                    key.as_deref(),
                    files,
                    &snapshot.config,
                    &snapshot.stubs,
                    &root,
                )
                .1
            })
            .await;
            let context = match resolved {
                Ok(Ok(context)) => context,
                Ok(Err(error)) => {
                    // The scan skips failed groups; here a failed group
                    // would keep the previous context indefinitely, so
                    // converge through the full scan instead (its commit
                    // resolves every group from current state). Removing
                    // the key would punish the group's siblings on a
                    // transient failure and disagree with the scan's own
                    // error behavior.
                    tracing::warn!(%error, "incremental workspace resolution degraded");
                    break;
                }
                Err(error) => {
                    tracing::warn!(%error, "incremental workspace resolution task failed");
                    // Infrastructure failure (the blocking task itself
                    // died): unsettled — the driver retries with a
                    // bounded stall budget instead of looping here.
                    return false;
                }
            };
            // Test seam: pause here (holding no lock, the resolved
            // context in hand) so a test can land a generation-moving
            // writer between this group's snapshot and its install —
            // the exact window the generation guard below exists for.
            #[cfg(feature = "test-util")]
            crate::test_seam::pause(Seam::ContextInstall).await;
            let newly_degraded = {
                let mut state = self.state.lock().await;
                if state.index_generation != generation {
                    // A newer writer landed during the blocking resolve;
                    // retry once from current state, else let the full
                    // scan converge. Drop the lock first: the retry
                    // re-snapshots below, and the scan backstop blocks.
                    drop(state);
                    continue;
                }
                if let Some(ctx) = state
                    .folder_contexts
                    .iter_mut()
                    .find(|ctx| ctx.root == *folder_root)
                {
                    ctx.workspace_contexts.insert(package_root.clone(), context);
                } else {
                    // The owning folder vanished between the snapshot
                    // and this install; the removal path already
                    // cleared and republished.
                    return true;
                }
                state.newly_degraded_scopes()
            };
            for (path, reason) in &newly_degraded {
                self.client
                    .log_message(MessageType::WARNING, reason.notice(path))
                    .await;
            }
            return true;
        }
        // Two lost races in a row: a full scan converges every group
        // through its own generation-guarded commit. Only an Installed
        // scan settles this group — the walk resolved and installed
        // every group's context from current disk. A superseded scan
        // leaves the duty to its supplanter, and a settled-without-
        // install pass (an errored walk, generation still held)
        // installed nothing: both stay unsettled for the reconciliation
        // driver, which retries the group instead of re-scanning once
        // per event.
        matches!(
            self.background_index_outcome().await,
            BackgroundIndexOutcome::Installed
        )
    }

    /// Debounce one project publication pass covering `paths`: every pass
    /// publishes all checked files, so a burst of requests coalesces into
    /// the pass of the latest generation, which drains every requested path.
    /// With nothing requested, only dropped URIs are reconciled.
    async fn request_pass(&self, paths: impl IntoIterator<Item = String>) {
        let generation = {
            let mut state = self.state.lock().await;
            state.pending_diag_paths.extend(paths);
            if state.pending_diag_paths.is_empty() {
                None
            } else {
                state.diag_generation = state.diag_generation.wrapping_add(1);
                Some(state.diag_generation)
            }
        };
        let Some(generation) = generation else {
            self.clear_dropped_diagnostics().await;
            return;
        };
        let backend = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            // Only the latest generation drains, atomically with its check.
            let requested = {
                let mut state = backend.state.lock().await;
                (state.diag_generation == generation)
                    .then(|| std::mem::take(&mut state.pending_diag_paths))
            };
            if let Some(requested) = requested {
                backend.publish_diagnostics(requested, generation).await;
                #[cfg(feature = "test-util")]
                crate::test_seam::signal(Seam::DiagnosticCycle);
            }
        });
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
/// cannot keep them raw.
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
/// config's own directory instead of the workspace folder.
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
    // `baseline_disk_reads`.
    #[cfg(feature = "test-util")]
    BASELINE_DISK_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ry_config::load_baseline(&resolved)
        .map(Some)
        .map_err(|e| format!("{e}"))
}

/// Load one analysis context per workspace folder, each with its
/// index-correlated editor settings or the global fallback, ordered for
/// longest-prefix ownership.
pub(super) fn load_folder_contexts(
    workspace_folders: &[(usize, PathBuf)],
    server_settings: &ServerSettings,
) -> Vec<FolderAnalysisContext> {
    let mut contexts: Vec<_> = workspace_folders
        .iter()
        .map(|(settings_idx, root)| {
            let settings = server_settings
                .settings
                .get(*settings_idx)
                .cloned()
                .unwrap_or_else(|| server_settings.global_settings.clone());
            FolderAnalysisContext::load(root.clone(), settings, None)
        })
        .collect();
    contexts.sort_by_key(|ctx| std::cmp::Reverse(ctx.root.as_os_str().len()));
    contexts
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

#[cfg(test)]
mod degraded_notice_tests {
    use super::*;

    #[test]
    fn unchanged_scope_does_not_repeat_and_repaired_scope_can_fail_again() {
        let path = PathBuf::from("/project/R/sysdata.rda");
        let failed = ry_workspace::WorkspaceContext {
            degraded_scopes: vec![(
                path.clone(),
                ry_workspace::InventoryFailure::ParserResourceLimit,
            )],
            ..Default::default()
        };
        let mut folder = FolderAnalysisContext::default();
        folder.workspace_contexts.insert(None, failed);
        let mut state = State {
            folder_contexts: vec![folder],
            ..Default::default()
        };
        assert_eq!(state.newly_degraded_scopes().len(), 1);
        assert!(state.newly_degraded_scopes().is_empty());

        state.folder_contexts[0]
            .workspace_contexts
            .insert(None, ry_workspace::WorkspaceContext::default());
        assert!(state.newly_degraded_scopes().is_empty());

        state.folder_contexts[0].workspace_contexts.insert(
            None,
            ry_workspace::WorkspaceContext {
                degraded_scopes: vec![(path, ry_workspace::InventoryFailure::ParserResourceLimit)],
                ..Default::default()
            },
        );
        assert_eq!(state.newly_degraded_scopes().len(), 1);
    }
}
