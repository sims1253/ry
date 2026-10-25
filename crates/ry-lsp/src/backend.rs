//! LSP backend: `Backend`, `State`, and the `LanguageServer` impl. Open
//! documents live in [`documents`]; index maintenance and publication run
//! in [`writer`].

mod documents;
mod handlers;
mod writer;

use documents::{CachedHints, Documents};
use writer::{IndexWriter, SettingsUpdate, Work};

use crate::diagnostics::{
    diagnostic_origin, diagnostic_to_lsp_with_source, make_ignore_action, make_ignore_file_action,
};
use crate::hints::collect_inlay_hints;
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

#[derive(Clone)]
pub(super) struct Backend {
    state: Arc<Mutex<State>>,
    writer: tokio::sync::mpsc::UnboundedSender<Work>,
}

#[derive(Default)]
pub(super) struct State {
    /// Open documents. Each joins the project so cross-file resolution sees
    /// unsaved edits.
    docs: Documents,
    /// Bumped by every document change and every publication pass. A pass
    /// publishes only while its own generation holds, and diagnostic data
    /// carries it so code actions can reject findings from older passes.
    diag_generation: u64,

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
    /// Whether `workspace/didChangeWatchedFiles` can be registered dynamically.
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
}

/// The config, editor settings, stubs, and package metadata of one folder,
/// so two roots defining the same package differently never collide.
#[derive(Clone, Default)]
pub(super) struct FolderAnalysisContext {
    pub root: PathBuf,
    /// Directory of the `ry.toml` the effective config was loaded from;
    /// `None` when defaults are in use. Config-relative `exclude` patterns,
    /// `include-build-ignored`, and baseline keys anchor here, as in the CLI.
    pub config_root: Option<PathBuf>,
    /// Discovered from `root`, or the editor's `configuration` path.
    pub config: ry_config::Config,
    pub folder_settings: FolderSettings,
    /// Local typeshed stubs from this folder's `ry.toml`.
    pub stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Resolution contexts keyed by package group: each package is its
    /// own library scope.
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
    /// Load `root`'s config, stubs, and baseline. A failed reload keeps
    /// `previous`'s config and baseline, and keeps its stubs when every
    /// configured directory failed. Deliberate removals always take effect.
    /// This reads disk, so callers run it outside the state lock.
    pub(super) fn load(
        root: PathBuf,
        folder_settings: FolderSettings,
        previous: Option<&FolderAnalysisContext>,
    ) -> Self {
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
        self.scoped_policy = ry_checker::ScopedRulePolicy::new(&self.config, &protected);
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

    /// Post-process through the pipeline `ry check` uses, so the editor
    /// sees what the CLI reports. Scoped rule severities match
    /// `policy_path`; without one, the base filter applies.
    fn publish(
        &self,
        diagnostics: Vec<ry_checker::Diagnostic>,
        path: &str,
        policy_path: Option<&Path>,
        file: &SourceFile,
        origin: Option<serde_json::Value>,
        related_uri: Option<&Url>,
    ) -> Vec<LspDiagnostic> {
        let diagnostics = self.post_process(diagnostics, path, policy_path, Some(file));
        to_lsp(&diagnostics, &file.source, related_uri, origin)
    }

    /// The RY000 for an open buffer ry cannot parse, which `ry check` lets
    /// through its severity filter and baseline like any file-level finding.
    fn publish_parse_failure(
        &self,
        path: &str,
        text: &str,
        error: ry_core::ParseError,
        origin: Option<serde_json::Value>,
    ) -> Vec<LspDiagnostic> {
        let error = ry_workspace::SourceError::Parse {
            error,
            text: text.to_owned(),
        };
        let diagnostic = ry_checker::frontend::parse_failure(path, &error);
        let diagnostics = self.post_process(vec![diagnostic], path, Some(Path::new(path)), None);
        to_lsp(&diagnostics, text, None, origin)
    }

    fn post_process(
        &self,
        diagnostics: Vec<ry_checker::Diagnostic>,
        path: &str,
        policy_path: Option<&Path>,
        file: Option<&SourceFile>,
    ) -> Vec<ry_checker::Diagnostic> {
        let filter = policy_path.map(|native| self.scoped_policy.filter_for(native, &self.filter));
        let post = ry_checker::PostProcess {
            filter: filter.as_deref().unwrap_or(&self.filter),
            baseline: self.baseline.as_ref(),
            min_confidence: self.min_confidence.unwrap_or(ry_checker::Confidence::Low),
            repo_root: self.anchor(),
        };
        let mut diagnostics = post.pre_demotion(diagnostics, path, file);
        post.post_demotion(&mut diagnostics);
        diagnostics
    }
}

/// Convert post-processed diagnostics against `source`, each stamped with
/// the publication's `origin`.
fn to_lsp(
    diagnostics: &[ry_checker::Diagnostic],
    source: &str,
    related_uri: Option<&Url>,
    origin: Option<serde_json::Value>,
) -> Vec<LspDiagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let mut diagnostic = diagnostic_to_lsp_with_source(diagnostic, source, related_uri);
            diagnostic.data = origin.clone();
            diagnostic
        })
        .collect()
}

#[derive(Default)]
pub(super) struct ProjectCache {
    project: Project,
    /// Version and snapshot of every installed file. Indexed files all
    /// carry version zero, so `Arc` identity also decides freshness.
    files: HashMap<String, (i32, Arc<SourceFile>)>,
}

pub(super) struct ProjectCheckResult {
    pub diagnostics: Vec<(String, Vec<ry_checker::Diagnostic>)>,
    /// The snapshots this check saw. Publication reads source and comments
    /// from these, never from a newer document.
    files: HashMap<String, Arc<SourceFile>>,
}

/// One file fed into a package check: its path, the open document's
/// version (zero for indexed disk files), and the parsed snapshot.
type PackageFile = (String, i32, Arc<SourceFile>);

impl ProjectCache {
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

    /// The innermost folder containing `doc_path`.
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

    /// Whether a `disk_files` entry counts against `budget_root`'s
    /// `index.max-files`. Like the walk, each entry counts for its
    /// innermost root only.
    fn entry_consumes_root_budget(&self, existing: &std::path::Path, budget_root: &Path) -> bool {
        existing.starts_with(budget_root)
            && !self.folder_contexts.iter().any(|ctx| {
                ctx.root.as_path() != budget_root
                    && ctx.root.starts_with(budget_root)
                    && existing.starts_with(&ctx.root)
            })
    }

    /// Whether the server analyzes `doc_path`. Beyond a disabled context,
    /// the owning folder's discovery rules apply (excludes, size and depth
    /// caps), so an open buffer cannot add definitions the index omitted.
    /// The size cap measures the open text, which can differ from disk.
    /// Closed paths came through the capped walk and skip it.
    fn eligibility_for_path(&self, doc_path: &str) -> bool {
        let content_len = self.docs.active(doc_path).map(|doc| doc.text.len() as u64);
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

    /// Whether any document open at `doc_path` is eligible, each judged by
    /// its own size, so the active text never decides for a sibling.
    fn any_open_source_eligible(&self, doc_path: &str) -> bool {
        match self.docs.at(doc_path) {
            [] => self.eligibility_for_path_with_len(doc_path, None),
            docs => docs.iter().any(|doc| {
                self.eligibility_for_path_with_len(doc_path, Some(doc.text.len() as u64))
            }),
        }
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
        serde_json::json!({"globPattern": "**/src/*.{c,cc,cpp,cxx,h,hh,hpp}"}),
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

/// Return the AST of the document `path` analyzes, with the exact text it
/// was parsed from, so position conversions always match the AST's spans.
/// Parsing runs outside the state lock.
async fn parsed_file(
    state: &Mutex<State>,
    path: &str,
) -> Result<(Arc<SourceFile>, String), Unparsed> {
    loop {
        let (edit, text, old_tree, native) = {
            let state = state.lock().await;
            let doc = state.docs.active(path).ok_or(Unparsed::Closed)?;
            if let Some(file) = &doc.parsed {
                return Ok((Arc::clone(file), doc.text.clone()));
            }
            (
                doc.edit,
                doc.text.clone(),
                doc.tree.clone(),
                doc.native.clone(),
            )
        };
        // A test can park here to land a newer edit before this parse
        // installs. The edit check below then rejects it.
        #[cfg(feature = "test-util")]
        let paused_parse = crate::test_seam::pause(Seam::Parse).await;
        let result = RParser::new().and_then(|mut parser| {
            ry_workspace::reports::parse_source_with_tree(
                &mut parser,
                path,
                &text,
                old_tree.as_ref(),
            )
        });
        let (mut parsed, new_tree) = match result {
            Ok(parsed) => parsed,
            Err(error) => return Err(Unparsed::Failed { text, error }),
        };
        parsed.native_path = native;
        let mut state = state.lock().await;
        // A concurrent parse of the same edit may have landed first. Keep
        // its AST, because callers compare ASTs by pointer.
        let stored = state
            .docs
            .active_mut(path)
            .filter(|doc| doc.edit == edit)
            .map(|doc| {
                if doc.parsed.is_none() && new_tree.is_some() {
                    doc.tree = new_tree;
                }
                Arc::clone(doc.parsed.get_or_insert_with(|| Arc::new(parsed)))
            });
        #[cfg(feature = "test-util")]
        if paused_parse {
            crate::test_seam::landed(Seam::Parse);
        }
        if let Some(file) = stored {
            return Ok((file, text));
        }
    }
}

/// Why [`parsed_file`] returned no AST.
#[derive(Debug)]
enum Unparsed {
    /// The path is not open.
    Closed,
    /// The parser rejected `text`, the document's text at the time.
    Failed {
        text: String,
        error: ry_core::ParseError,
    },
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

    /// Return hints from one current parse and its assignment-site inference.
    async fn hints_for(&self, path: &str) -> Option<Vec<InlayHint>> {
        let user_stubs = {
            let state = self.state.lock().await;
            let stubs = &state.context_for_path(path).stubs;
            if let Some(cached) = state.docs.active(path).and_then(|doc| doc.hints.as_ref())
                && Arc::ptr_eq(&cached.stubs, stubs)
            {
                return Some(cached.hints.clone());
            }
            Arc::clone(stubs)
        };
        let (file, text) = parsed_file(&self.state, path).await.ok()?;
        let mut checker = ry_checker::Checker::new(path);
        checker.set_user_stubs(Arc::clone(&user_stubs));
        checker.enable_assignment_capture();
        checker.check(&file);
        let hints = collect_inlay_hints(&checker.take_assignment_types(), &text);
        let mut state = self.state.lock().await;
        if !Arc::ptr_eq(&state.context_for_path(path).stubs, &user_stubs) {
            return None;
        }
        let doc = state.docs.active_mut(path).filter(|doc| {
            doc.parsed
                .as_ref()
                .is_some_and(|parsed| Arc::ptr_eq(parsed, &file))
        })?;
        doc.hints = Some(CachedHints {
            stubs: user_stubs,
            hints: hints.clone(),
        });
        Some(hints)
    }
}

fn custom_config_paths(state: &State) -> Vec<PathBuf> {
    let mut paths: Vec<_> = state
        .folder_contexts
        .iter()
        .filter_map(|ctx| {
            let path = ctx.folder_settings.configuration.as_ref()?;
            Some(normalize_path(&ctx.root.join(path)))
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

/// Remove dot segments (`config/../config` becomes `config`) without
/// touching the filesystem, as client event URIs do. Config anchors and
/// watched config paths must match those clean absolute paths.
fn normalize_path(path: &Path) -> PathBuf {
    Url::from_file_path(path)
        .ok()
        .and_then(|uri| Url::parse(uri.as_str()).ok())
        .and_then(|uri| uri.to_file_path().ok())
        .unwrap_or_else(|| path.to_path_buf())
}

/// Load the folder's explicit configuration or its nearest `ry.toml`, with
/// the directory it came from (`None` for defaults). Like the CLI's config
/// root, that directory anchors excludes and baseline keys. Errors reach
/// the caller so a reload can keep the last valid config. Reads disk.
fn discover_folder_config(
    folder_settings: &FolderSettings,
    folder_root: &std::path::Path,
) -> std::result::Result<(Option<PathBuf>, ry_config::Config), ry_config::ConfigError> {
    if let Some(config_path) = &folder_settings.configuration {
        let path = folder_root.join(config_path);
        let config = ry_config::Config::load_file(&path)?;
        return Ok((path.parent().map(normalize_path), config));
    }
    match ry_config::Config::discover(folder_root)? {
        Some((path, config)) => Ok((path.parent().map(normalize_path), config)),
        None => Ok((None, ry_config::Config::default())),
    }
}

/// Load the baseline named by the editor settings or `ry.toml`. `Ok(None)`
/// means none is configured. `Err` lets a reload keep the last valid one.
/// Reads disk.
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
    ry_config::load_baseline(&resolved)
        .map(Some)
        .map_err(|e| format!("{e}"))
}

/// Load one context per workspace folder with its editor settings (by
/// index, else the global ones), innermost roots first.
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

/// The URI for a display path. Non-file keys such as `untitled:` URIs
/// parse back as themselves.
pub(crate) fn path_to_uri(path: &str) -> Url {
    Url::from_file_path(path).unwrap_or_else(|_| {
        Url::parse(path).unwrap_or_else(|_| Url::parse("file:///unknown").unwrap())
    })
}

/// The display path the checker and the disk index key a URI by. A
/// non-UTF-8 filename becomes lossy. A non-file URI keys by its own text.
pub(crate) fn uri_to_path(uri: &Url) -> String {
    uri.to_file_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| uri.as_str().to_string())
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
