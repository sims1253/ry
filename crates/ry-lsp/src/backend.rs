//! LSP backend: `Backend`, `State`, the `LanguageServer` impl, and the
//! document cache. Index maintenance and publication run in [`writer`].

mod handlers;
mod writer;

use writer::{IndexWriter, SettingsUpdate, Work};

use crate::diagnostics::{
    diagnostic_origin, diagnostic_to_lsp_with_source, make_ignore_action, make_ignore_file_action,
};
use crate::hints::collect_inlay_hints;
use crate::positions::{byte_offset_to_point, position_to_byte_offset};
use crate::settings::{FolderSettings, ServerSettings};
#[cfg(feature = "test-util")]
use crate::test_seam::Seam;

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

/// Canonical paths of the baseline files read by `load_folder_baseline`,
/// the only baseline disk-read site in the LSP, so tests can assert the
/// publish path performs none.
#[cfg(feature = "test-util")]
static BASELINE_DISK_READS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Number of reads of the baseline file at `path` since process start. A
/// publish/inlay-hint that does not change this value performs zero
/// baseline disk I/O. Counted per file so tests running in parallel, each
/// with its own fixture, never observe each other's reads.
#[cfg(feature = "test-util")]
pub fn baseline_disk_reads(path: &Path) -> usize {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    BASELINE_DISK_READS
        .lock()
        .unwrap()
        .iter()
        .filter(|read| **read == path)
        .count()
}

#[derive(Clone)]
pub(super) struct Backend {
    state: Arc<Mutex<State>>,
    writer: tokio::sync::mpsc::UnboundedSender<Work>,
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
    /// Bumped by every document change and every publication pass. A pass
    /// publishes only while its own generation holds, and diagnostic data
    /// carries it so code actions can reject findings from older passes.
    diag_generation: u64,
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
    /// Whether the client accepts `relatedInformation` on published diagnostics.
    supports_related_information: bool,
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
        related_uri: Option<&Url>,
    ) -> Vec<LspDiagnostic> {
        let filter = policy_path.map(|native| self.scoped_policy.filter_for(native, &self.filter));
        let post = ry_checker::PostProcess {
            filter: filter.as_deref().unwrap_or(&self.filter),
            baseline: self.baseline.as_ref(),
            min_confidence: self.min_confidence.unwrap_or(ry_checker::Confidence::Low),
            repo_root: self.anchor(),
        };
        let mut diagnostics = post.pre_demotion(diagnostics, path, Some(file));
        post.post_demotion(&mut diagnostics);
        diagnostics
            .iter()
            .map(|diagnostic| {
                let mut diagnostic =
                    diagnostic_to_lsp_with_source(diagnostic, &file.source, related_uri);
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

/// Register the workspace watchers for the current folder settings,
/// replacing an earlier registration whose custom config paths changed.
async fn refresh_watchers(client: Client, state: Arc<Mutex<State>>) {
    let registered = Arc::clone(&state.lock().await.watcher_paths);
    // Serialize replacement registrations without holding the document lock.
    let mut registered = registered.lock().await;
    let (paths, relative) = {
        let state = state.lock().await;
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
        if let Err(error) = client
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
        let pattern =
            if relative {
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
    match client.register_capability(vec![registration]).await {
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
async fn parsed_file(state: &Mutex<State>, path: &str) -> Option<(Arc<SourceFile>, String)> {
    loop {
        // One guard reads the parse cache, the document text/version,
        // and the old tree as one snapshot; parsing happens outside
        // the lock. Clone the old tree only on a cache miss.
        let (text, version, old_tree, native) = {
            let state = state.lock().await;
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
        let mut state = state.lock().await;
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

impl Backend {
    /// A backend with empty state and its index writer running.
    pub(super) fn new(client: Client) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (writer, work) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(IndexWriter::new(client, Arc::clone(&state), work).run());
        Backend { state, writer }
    }

    /// Queue work for the index writer. Handlers that changed document
    /// state call this while still holding the state lock, so a pass never
    /// observes the change without its queued follow-up.
    fn send(&self, work: Work) {
        // The writer stops only at shutdown, after which nothing is owed.
        let _ = self.writer.send(work);
    }

    async fn parsed_file(&self, path: &str) -> Option<(Arc<SourceFile>, String)> {
        parsed_file(&self.state, path).await
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
    BASELINE_DISK_READS
        .lock()
        .unwrap()
        .push(resolved.canonicalize().unwrap_or_else(|_| resolved.clone()));
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
