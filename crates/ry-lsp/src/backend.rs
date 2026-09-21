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

/// What one pending watched/close obligation still owes; see
/// [`State::pending_refreshes`]. The phases settle in order — bytes
/// (or a confirmed removal) first, then the owning package's
/// resolution context together with the publication that must follow
/// it — because publishing before the context settles would show an
/// analysis built on a stale import context, which is not "converged"
/// either.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RefreshDuty {
    /// Indexed bytes (or a confirmed removal), the owning package's
    /// resolution context, and the publication that follows.
    BytesContextPublication,
    /// The bytes phase settled — a refresh landed an install/removal
    /// under its claim — leaving the context refresh and the
    /// publication that must follow it.
    ContextPublication,
}

/// One retained reconciliation obligation: the refresh epoch that
/// claimed it (the revision an acknowledgment must not exceed) and the
/// duty still owed. Whether a refresh for the path is currently in
/// flight (claimed but not yet returned) lives in
/// [`State::refreshes_in_flight`], outside this map, so cancelling an
/// obligation cannot disturb the in-flight accounting. The driver never
/// dispatches for a path with an in-flight refresh — preempting it
/// would claim a newer epoch and supersede a read that may still land,
/// the exact waste #538's start-order rule exists to prevent — so a
/// losing refresh's exit is what re-wakes the driver for its path.
#[derive(Clone, Copy)]
struct PendingRefresh {
    epoch: u64,
    duty: RefreshDuty,
}

/// Driver rounds without any obligation retiring before the driver
/// starts PACING itself; see
/// [`State::reconcile_rounds_without_progress`]. A round retires an
/// obligation when one is removed from `pending_refreshes` by settling
/// (a terminal outcome or a completed context-and-publication phase) —
/// measured by the monotonic [`State::retired_refreshes`] counter, not
/// by the map shrinking, because a mid-round event arrival offsets a
/// real retirement in a length comparison while retiring nothing
/// itself. Reaching the bound RETAINS every obligation: the driver
/// yields for the backoff [`reconcile_pace_delay`] computes and then
/// retries, so a burst of supersessions longer than any fixed retry
/// budget still converges once the churn stops. The bound paces the
/// work performed per unit time — rounds per wall-clock, and at most
/// the scans a round's own refresh ladder escalates to — it never
/// retires the obligation itself: a finite stall streak is not
/// evidence that completion became impossible. The map empties only
/// by settling, shutdown, or removal of the owning workspace.
/// Generous enough that ordinary churn never trips it — every
/// delivered event is finite, and under silence a round always lands
/// because nothing else moves the index generation while the driver
/// re-reads.
const RECONCILE_ROUNDS_BEFORE_PACING: u32 = 8;

/// The first paced delay, in milliseconds; see
/// [`RECONCILE_ROUNDS_BEFORE_PACING`]. Each further consecutive paced
/// round doubles it, capped at [`RECONCILE_PACE_CAP_MILLIS`], so
/// rounds per wall-clock stay bounded while completion remains
/// impossible. Both ends stay deliberately small — tens of
/// milliseconds to a cap well under a second: each paced round delays
/// work by at most the cap (on the order of, and eventually
/// exceeding, the ~180ms `schedule_diagnostics` debounce), the
/// cumulative convergence delay grows with the episode's length, and
/// any retirement resets the ladder.
const RECONCILE_PACE_BASE_MILLIS: u64 = 40;

/// The paced-delay cap, in milliseconds; see
/// [`RECONCILE_PACE_BASE_MILLIS`].
const RECONCILE_PACE_CAP_MILLIS: u64 = 320;

/// The delay a paced driver round yields for: the base doubled once
/// per consecutive paced round beyond the first, capped. Pure
/// function of the stall counter so tests can pin each rung exactly.
fn reconcile_pace_delay(rounds_without_progress: u32) -> std::time::Duration {
    // The ladder's shape is pinned at compile time: three doublings of
    // the base must reach the cap exactly, so the `.min(3)` below names
    // the cap's rung and no rung can overshoot into the clamp. Editing
    // either constant independently now fails to compile.
    const _: () = assert!(RECONCILE_PACE_BASE_MILLIS << 3 == RECONCILE_PACE_CAP_MILLIS);
    let doublings = rounds_without_progress
        .saturating_sub(RECONCILE_ROUNDS_BEFORE_PACING)
        // The base already sits at rung 0; three doublings reach the
        // cap, and every later paced round stays there.
        .min(3);
    let millis = (RECONCILE_PACE_BASE_MILLIS << doublings).min(RECONCILE_PACE_CAP_MILLIS);
    std::time::Duration::from_millis(millis)
}

/// How one finished driver round folds into the pacing bookkeeping;
/// see [`State::reconcile_round_progress`].
enum ReconcileRoundProgress {
    /// The round retired at least one obligation (or emptied the map
    /// through a mid-round cancellation — root removal or shutdown —
    /// which the round head turns into the idle transition): real
    /// progress or a legitimate terminal state, resetting the stall
    /// count.
    Progress,
    /// The round retired nothing, the stall count is still below
    /// [`RECONCILE_ROUNDS_BEFORE_PACING`]: drive on unimpeded.
    Stalled,
    /// The round retired nothing and the stall count reached the
    /// pacing bound: every obligation is RETAINED and the driver
    /// yields for `delay` before its next round. `warn` is set exactly
    /// on the round that enters the stall episode (the count's first
    /// arrival at the bound), so the visible warning fires once per
    /// episode; any retirement resets the count and re-arms the
    /// episode boundary.
    Pacing {
        delay: std::time::Duration,
        warn: bool,
    },
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
    /// Per-path refresh epoch (#538): the `refresh_epoch_counter` value
    /// claimed when the most recent `refresh_disk_entry` for the path
    /// STARTED, before its blocking read. A refresh's commit must still
    /// hold its claimed epoch next to the generation check — tower-lsp
    /// dispatches watched-file handlers concurrently, so two refreshes
    /// for one path can snapshot the same generation, and the
    /// generation alone would let the older read win whenever it
    /// commits first (it lands stale bytes and bumps the generation,
    /// making the newer read's commit look stale). Values come from one
    /// global counter, so an entry removed with a landed removal (or a
    /// budget refusal, which likewise passed the epoch check) is
    /// re-seeded later with a fresh value that cannot alias a live
    /// claim short of the u64 wrap the index generation accepts.
    refresh_epochs: HashMap<String, u64>,
    /// Monotonic source of `refresh_epochs` values; see there (#538).
    refresh_epoch_counter: u64,
    /// P1 (#551 successor): per-path reconciliation obligations. A
    /// watched/close event's duty is not "the refresh I dispatched" but
    /// "final analysis converges to the current disk state": the
    /// per-file refresh ladder can lose both generation races AND its
    /// full-scan backstop (superseded by yet another landing), and a
    /// losing fallback used to return while forgetting the path
    /// entirely — with no open document and no further event, the fix
    /// or creation stranded forever. Each entry is enqueued at the
    /// refresh's epoch claim, BEFORE the blocking read that might lose,
    /// keyed by path so a burst coalesces into the latest claim, and
    /// acknowledged only when the acknowledgment's epoch still covers
    /// the entry, so a newer event arriving mid-read keeps its own
    /// fresh obligation. See [`RefreshDuty`] for the phases.
    pending_refreshes: HashMap<String, PendingRefresh>,
    /// Per-path count of refreshes claimed but not yet returned,
    /// keyed like `refresh_epochs`. Kept OUTSIDE
    /// `pending_refreshes` on purpose: obligations are cancelled
    /// wholesale (folder removal, shutdown) while their refreshes are
    /// still in flight, and a later event can re-enqueue a fresh
    /// obligation for the same path before the old refresh returns —
    /// settling by path against the obligation map would let the old
    /// refresh's exit decrement the NEW obligation's counter, an
    /// undercount the driver would read as "idle" before preempting
    /// the in-flight read, the exact waste the counter exists to
    /// prevent. The only writers are the claim and the exit inside
    /// `refresh_disk_entry`, so the count always equals
    /// claims-minus-returns regardless of what happened to the
    /// obligations. Like `refresh_epochs`, entries return to zero (and
    /// leave) as their refreshes exit, never on cancellation.
    refreshes_in_flight: HashMap<String, u32>,
    /// Monotonic count of reconciliation obligations RETIRED — removed
    /// from `pending_refreshes` by settling, not by cancellation. The
    /// driver's progress measurement compares this across a round:
    /// folder removals and shutdown cancel obligations without
    /// retiring them, so they do not count as the driver's progress
    /// (each is a one-shot external event that shrinks the map toward
    /// the driver's exit, not work the driver could stall against),
    /// and the pacing bound never removes anything — a stalled round
    /// retains its obligations and only slows the driver down.
    retired_refreshes: u64,
    /// True while the reconciliation driver task is between its spawn
    /// and its idle transition. The idle transition and every
    /// enqueue-side wakeup share the state lock, so an obligation
    /// cannot appear between the driver's last duty check and its
    /// flag clear: the enqueue either sees the flag still set (the
    /// running driver re-checks before idling) or finds it clear
    /// and spawns a driver itself. At most one driver runs at a time.
    reconcile_driver_active: bool,
    /// Consecutive driver rounds that retired no obligation; bounds
    /// the driver's pace under pathological environments (persistent
    /// I/O failure, unending index churn) — after
    /// [`RECONCILE_ROUNDS_BEFORE_PACING`] such rounds the driver
    /// RETAINS every obligation, warns once per stall episode, and
    /// yields for the capped backoff [`reconcile_pace_delay`]
    /// computes before retrying, instead of spinning (or
    /// full-scanning) forever. The obligation's lifetime is never
    /// bounded: only settling, shutdown, or removal of the owning
    /// workspace empties the map. Progress is a retired obligation
    /// (see [`State::retired_refreshes`]), so events arriving
    /// mid-round cannot mask the rounds' own retirements.
    reconcile_rounds_without_progress: u32,
    /// Set by `shutdown`: the session is ending, so no reconciliation
    /// work may be claimed, dispatched, or published afterwards. The
    /// flag and the obligation purge are one critical section, and
    /// every claim/dispatch/publication site re-checks it under the
    /// state lock, so a driver already inside a round cannot resurrect
    /// work or client publications for the ended session.
    shutting_down: bool,
    /// Files opened during initialization wait for the first workspace context.
    initial_index_pending: bool,
    /// Runtime stubs loaded from the workspace's `ry.toml`. Kept in state so
    /// every rebuilt Project and single-file scope check sees the same data.
    user_stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Persistent multi-file checkers used only by diagnostics, one per
    /// package root for files no workspace folder owns (see
    /// [`FolderAnalysisContext::package_caches`] for the folder-owned
    /// equivalent). Each cache has its own mutex so package checks stay
    /// serialized without holding the document-state lock used by
    /// latency-sensitive LSP requests.
    root_caches: HashMap<Option<PathBuf>, Arc<Mutex<ProjectCache>>>,
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
    /// per-package project caches (see
    /// [`FolderAnalysisContext::package_caches`]).
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
    /// Per-package workspace resolution contexts, keyed by nearest
    /// `DESCRIPTION` ancestor root (`None` for files outside any package).
    /// Each R package is a separate library scope, so the folder-wide
    /// union used to leak one package's `library()` attachments and
    /// package metadata into its siblings; resolving per package keeps
    /// the per-package boundary `ry check` partitions on (#487).
    pub workspace_contexts: HashMap<Option<PathBuf>, ry_workspace::WorkspaceContext>,
    /// The baseline loaded from `ry.toml`/editor settings, cached during
    /// context construction so the publish path performs no disk access.
    pub baseline: Option<ry_config::Baseline>,
    /// Severity filter compiled once during context construction.
    pub filter: ry_checker::SeverityFilter,
    /// Precomputed minimum confidence threshold.
    pub min_confidence: Option<ry_checker::Confidence>,
    /// Precompiled exclude glob patterns.
    pub excludes: ry_config::Excludes,
    /// This folder's per-package project caches for isolated checking.
    /// Each workspace folder gets one `ProjectCache` per package root (the
    /// `None` key covers non-package scripts) so two packages nested under
    /// one folder never share a `Project`: pooling them let top-level
    /// bindings and inferred functions leak between namespaces, hiding
    /// real RY010 findings and resolving same-named functions to the wrong
    /// package's definition (#487). Each cache is shared via `Arc` and the
    /// map is carried across context rebuilds so incremental check state
    /// survives a config reload.
    pub package_caches: HashMap<Option<PathBuf>, Arc<Mutex<ProjectCache>>>,
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

/// One file fed into a package check: its path, the open document's
/// version (zero for indexed disk files), and the parsed snapshot.
type PackageFile = (String, i32, Arc<SourceFile>);

/// Project files owned by one folder, sub-partitioned by package root:
/// the owning folder context (if any), the nearest-`DESCRIPTION`
/// ancestor root (`None` for files outside any package), and the files.
/// Each R package is a separate library scope and checks through its own
/// `ProjectCache` (see [`FolderAnalysisContext::package_caches`).
struct PackagePartition {
    ctx: Option<FolderAnalysisContext>,
    package_root: Option<PathBuf>,
    files: Vec<PackageFile>,
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
    /// Settle the pending obligation for `path` as of `epoch`: a
    /// landing keeps the entry with only the context-and-publication
    /// duty owed; a terminal no-landing outcome (open-buffer
    /// ownership, cap refusal) removes it entirely, because nothing a
    /// later close or cap change won't re-enqueue remains owed. Only
    /// entries the epoch still covers move — a newer event's claim was
    /// inserted at its own, higher epoch and keeps its full duty — so
    /// an older or unrelated writer never acknowledges another
    /// revision's obligation (#538's start-order rule, applied to the
    /// bookkeeping). A removal here RETIRES the obligation: the
    /// monotonic `retired_refreshes` count moves, which is the driver's
    /// progress signal.
    fn settle_pending_refresh(&mut self, path: &str, epoch: u64, landed: bool) {
        let Some(pending) = self.pending_refreshes.get_mut(path) else {
            return;
        };
        if pending.epoch > epoch {
            return;
        }
        if landed {
            pending.duty = RefreshDuty::ContextPublication;
        } else {
            self.pending_refreshes.remove(path);
            self.retired_refreshes = self.retired_refreshes.wrapping_add(1);
        }
    }

    /// Claim the path's next refresh epoch and enqueue (or re-arm) its
    /// reconciliation obligation, together with the in-flight slot for
    /// the claiming refresh — one critical section so a concurrent
    /// driver or shutdown decision cannot observe the claim without its
    /// bookkeeping. Returns the claimed epoch, or `None` when the
    /// session is shutting down: no post-shutdown claim may re-seed the
    /// maps that `shutdown` left empty.
    fn claim_refresh_epoch(&mut self, path: &str) -> Option<u64> {
        if self.shutting_down {
            return None;
        }
        self.refresh_epoch_counter = self.refresh_epoch_counter.wrapping_add(1);
        let refresh_epoch = self.refresh_epoch_counter;
        self.refresh_epochs.insert(path.to_string(), refresh_epoch);
        *self
            .refreshes_in_flight
            .entry(path.to_string())
            .or_insert(0) += 1;
        let pending = self
            .pending_refreshes
            .entry(path.to_string())
            .or_insert(PendingRefresh {
                epoch: refresh_epoch,
                duty: RefreshDuty::BytesContextPublication,
            });
        pending.epoch = refresh_epoch;
        pending.duty = RefreshDuty::BytesContextPublication;
        Some(refresh_epoch)
    }

    /// Record that one refresh for `path` finished (its claim's
    /// counterpart to the enqueue-side in-flight increment, kept
    /// independent of the obligation map's removals). Returns whether
    /// an obligation remains for the path AND no refresh is in flight
    /// for it — the condition under which the reconciliation driver
    /// may take over.
    fn note_refresh_exit(&mut self, path: &str) -> bool {
        if let Some(in_flight) = self.refreshes_in_flight.get_mut(path) {
            *in_flight = in_flight.saturating_sub(1);
            if *in_flight == 0 {
                self.refreshes_in_flight.remove(path);
            }
        }
        self.pending_refreshes.contains_key(path) && !self.refreshes_in_flight.contains_key(path)
    }

    /// Whether some pending obligation is drivable right now: the
    /// session is not shutting down and at least one owed path has no
    /// refresh in flight. The driver's idle transition re-derives its
    /// verdict under the same lock hold that clears the active flag,
    /// because the duty checks earlier in a round ran under separate,
    /// earlier lock holds — an in-flight refresh can exit (and its
    /// dispatcher's epilogue wake can find the flag still set, waking
    /// nobody) between the last duty check and the clear.
    fn has_drivable_obligation(&self) -> bool {
        !self.shutting_down
            && self
                .pending_refreshes
                .iter()
                .any(|(path, _)| !self.refreshes_in_flight.contains_key(path))
    }

    /// Fold one finished driver round into the pacing bookkeeping:
    /// `retired_before` is the monotonic retirement count snapshotted
    /// at the round's start. A round that retired at least one
    /// obligation resets the stall count (arrivals during the round
    /// are irrelevant — they retire nothing and are owed work
    /// themselves); a round that retired nothing increments it, and
    /// reaching [`RECONCILE_ROUNDS_BEFORE_PACING`] retains every
    /// surviving obligation and reports the paced delay (with the
    /// once-per-episode warning flag) instead of dropping anything.
    fn reconcile_round_progress(&mut self, retired_before: u64) -> ReconcileRoundProgress {
        if self.retired_refreshes != retired_before {
            self.reconcile_rounds_without_progress = 0;
            return ReconcileRoundProgress::Progress;
        }
        if self.pending_refreshes.is_empty() {
            // Emptied mid-round by a cancellation (root removal,
            // shutdown), not by this round's work: the round head
            // performs the idle transition on the next pass, so there
            // is nothing left to pace.
            return ReconcileRoundProgress::Progress;
        }
        // Saturating, not wrapping: the counter feeds the ladder's
        // rung comparisons, and a wrapped value would corrupt them.
        // Saturating at the ceiling is the correct asymptote anyway —
        // a permanently pathological session stays at the cap delay —
        // and the ceiling is unreachable on any real clock (2^32
        // rounds at the 40ms floor is years).
        self.reconcile_rounds_without_progress =
            self.reconcile_rounds_without_progress.saturating_add(1);
        if self.reconcile_rounds_without_progress < RECONCILE_ROUNDS_BEFORE_PACING {
            return ReconcileRoundProgress::Stalled;
        }
        ReconcileRoundProgress::Pacing {
            delay: reconcile_pace_delay(self.reconcile_rounds_without_progress),
            warn: self.reconcile_rounds_without_progress == RECONCILE_ROUNDS_BEFORE_PACING,
        }
    }

    /// Complete the final phase of a landing obligation — the context
    /// refresh settled and the publication is scheduled — removing the
    /// entry (and retiring it: the completion is the driver's progress
    /// signal). `epoch` is the completion TOKEN: the refresh epoch of
    /// the landing whose context-and-publication work just finished,
    /// snapshotted from that refresh's own claim before its read —
    /// never a lookup of whatever is current at acknowledgement time.
    /// The entry's epoch is the revision whose work is still owed, so
    /// the entry retires only when the completed work demonstrably
    /// covers it: `duty == ContextPublication && pending.epoch <=
    /// epoch`. A newer event re-arms the entry at its own, higher
    /// epoch, so an older completion can never retire it — the newer
    /// event may have advanced the entry to `ContextPublication`, but
    /// matching the phase name is not proof the older operation
    /// incorporated the newer bytes; the newer refresh's own
    /// dispatcher (or the reconciliation driver, once that refresh's
    /// context attempt is superseded) owes the follow-up. Conversely a
    /// newer completion retires an older entry legitimately: it read
    /// and landed after the older claim, so its context pass covered
    /// everything the older revision owed. If the entry was already
    /// retired, the acknowledgement is a no-op.
    ///
    /// INVARIANT on the `<=`: epochs come from one strictly increasing
    /// counter ([`State::refresh_epoch_counter`]), so claim order is a
    /// total order for the process's lifetime. The counter's
    /// `wrapping_add` bounds that claim the same way
    /// [`State::index_generation`] and [`State::refresh_epochs`]
    /// already document and accept: a misorder needs a full 2^64-claim
    /// wrap between one obligation's claim and its acknowledgement —
    /// the same u64 wrap those counters' epoch/generation checks accept
    /// rather than defend against, because no session approaches it.
    fn complete_pending_publication(&mut self, path: &str, epoch: u64) {
        if self.pending_refreshes.get(path).is_some_and(|pending| {
            pending.duty == RefreshDuty::ContextPublication && pending.epoch <= epoch
        }) {
            self.pending_refreshes.remove(path);
            self.retired_refreshes = self.retired_refreshes.wrapping_add(1);
        }
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

    /// Whether an indexed `disk_files` entry consumes `budget_root`'s
    /// `index.max-files` budget: entries attribute to their INNERMOST
    /// containing root (longest-prefix, the same ownership
    /// [`folder_context_for_path`](Self::folder_context_for_path)
    /// applies), so in a nested multi-root workspace a file under an
    /// inner root counts toward the inner root's cap — never the
    /// outer's — matching how the walk caps each root's scan
    /// independently (#525). `Path::starts_with` is already
    /// component-wise; the innermost rule is what the plain prefix
    /// count gets wrong.
    fn entry_consumes_root_budget(&self, existing: &std::path::Path, budget_root: &Path) -> bool {
        existing.starts_with(budget_root)
            && !self.folder_contexts.iter().any(|ctx| {
                ctx.root.as_path() != budget_root
                    && ctx.root.starts_with(budget_root)
                    && existing.starts_with(&ctx.root)
            })
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

/// How a background index pass ended. `Installed` is the only outcome
/// that put new bytes in `disk_files`; `SettledWithoutInstall` settled
/// the pass's index duty (clearing `initial_index_pending`) while
/// leaving the map untouched — an empty-roots pass or a walk that
/// errored while its generation still held; `Superseded` lost the
/// generation race and the supplanter owns the state.
enum BackgroundIndexOutcome {
    Installed,
    SettledWithoutInstall,
    Superseded,
}

/// Spawn one reconciliation driver task. A plain function boundary, not
/// an inline `tokio::spawn` at the wake site: the driver's future
/// contains the refresh ladder, whose exit-side wake schedules the
/// driver again, so an inline spawn would make the compiler's Send
/// obligations circular (the driver's future must be Send because it is
/// spawned, contains the wake, whose spawn again requires the driver's
/// future Send). The synchronous call boundary keeps the wake's future
/// free of the spawned future, breaking the cycle at the cost of one
/// opaque call.
fn spawn_reconciliation_driver(backend: Backend) {
    tokio::spawn(async move {
        backend.run_reconciliation().await;
    });
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
            // R source, so an edit/create/delete of an unopened file
            // reaches `did_change_watched_files` instead of sitting stale
            // in the disk index until an unrelated rescan (#486). The
            // extension set mirrors `ry_workspace` discovery (conventional
            // `.R`/`.r` plus the historical S-dialect spellings); per-file
            // walk rules (excludes, fixtures, caps) are applied when the
            // event is processed, not in the glob.
            serde_json::json!({"globPattern": "**/*.{R,r,S,s,q}"}),
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
        // One path-keyed source view (#490): Project shadowing follows
        // insertion order (the last file wins), so the assembly must not
        // let open/closed status decide precedence. Start from the
        // eligible disk entries, remove the disk counterpart of EVERY
        // open path, insert each eligible open buffer at its own path,
        // and sort the unified collection once by the canonical path
        // ordering the CLI's sorted discovery uses. An open buffer is
        // authoritative for its own path only: it replaces its path's
        // disk bytes (same-path authority) but no longer outranks a
        // different closed file that sorts after it — layering every
        // open document after every disk entry made merely opening a
        // byte-identical file flip the winning same-named definition,
        // a change no edit and no CLI run could reproduce. An INELIGIBLE
        // open buffer still suppresses its (eligible) stale disk twin —
        // not just the eligible ones — so a buffer that grew past
        // `max-file-bytes` cannot keep contributing its path through the
        // small on-disk twin the index still holds (#488): the open
        // buffer owns its path, eligible or not, and an ineligible one
        // contributes nothing at all. Files in disabled folders are
        // dropped by the same eligibility rule on both sides.
        let mut project_files: Vec<(String, i32, Arc<SourceFile>)> = {
            let state = self.state.lock().await;
            let open_paths: std::collections::HashSet<&str> =
                state.docs.keys().map(String::as_str).collect();
            state
                .disk_files
                .iter()
                .filter(|(p, _)| state.eligibility_for_path(p))
                .filter(|(p, _)| !open_paths.contains(p.as_str()))
                .map(|(p, file)| (p.clone(), 0, Arc::clone(file)))
                .collect::<Vec<_>>()
        };
        project_files.extend(open_files);
        // Paths are unique across the two sources — every open path's
        // disk twin was removed above — so one stable sort by path is
        // the whole merge; the folder/package partitioning below keeps
        // this relative order within each group.
        project_files.sort_by(|a, b| a.0.cmp(&b.0));

        // Check each package partition independently through its own
        // ProjectCache, stubs, and workspace context. The root-level
        // filter, confidence, exclude, baseline, and root state rides
        // along for the files no folder owns.
        let (
            folder_contexts,
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
                state.root_filter.clone(),
                state.root_min_confidence,
                state.root_excludes.clone(),
                state.root_baseline.clone(),
                state.root.clone(),
                state.root_config_dir.clone(),
            )
        };

        // Partition project_files by folder root, then sub-partition each
        // folder's files by nearest-`DESCRIPTION` ancestor root — the
        // shared `ry_workspace::group_by_package_root` boundary `ry check`
        // partitions on. Each R package is a separate library scope, so
        // sibling packages nested under one folder must not share one
        // `Project`: pooling them lets top-level bindings and inferred
        // functions leak between namespaces, which can both hide real
        // RY010 findings and resolve same-named functions to the wrong
        // package's definition (#487). Files no folder owns go to the
        // root project. The contexts are already clones, so every
        // partition carries its owning context instead of a map key
        // nobody reads.
        let mut folder_files: Vec<(Option<PathBuf>, Vec<PackageFile>)> = Vec::new();
        for (fp, ver, file) in project_files {
            let folder_root = folder_contexts
                .iter()
                .find(|c| std::path::Path::new(&fp).starts_with(&c.root))
                .map(|c| c.root.clone());
            // The ownership rule matches `folder_context_for_path`.
            match folder_files
                .iter_mut()
                .find(|(owned, _)| *owned == folder_root)
            {
                Some((_, files)) => files.push((fp, ver, file)),
                None => folder_files.push((folder_root, vec![(fp, ver, file)])),
            }
        }
        let mut partitions: Vec<PackagePartition> = Vec::new();
        for (folder_root, files) in &folder_files {
            let ctx = folder_root.as_ref().and_then(|folder_root| {
                folder_contexts
                    .iter()
                    .find(|c| &c.root == folder_root)
                    .cloned()
            });
            // The grouping returns indices into the folder's file list in
            // ascending order, so each package's files keep the canonical
            // sorted order the shadowing contract needs (#490).
            let paths: Vec<&str> = files.iter().map(|(path, _, _)| path.as_str()).collect();
            for (package_root, indices) in ry_workspace::group_by_package_root(paths) {
                partitions.push(PackagePartition {
                    ctx: ctx.clone(),
                    package_root,
                    files: indices.iter().map(|index| files[*index].clone()).collect(),
                });
            }
        }

        // Snapshot each partition's check inputs under the state lock:
        // stubs, the package's own workspace context, and its project
        // cache handle. Cache handles are created on first use and live in
        // the owning folder's map (or the root map), so incremental check
        // state survives across passes. A partition whose folder vanished
        // mid-pass falls back to the root inputs rather than skipping the
        // check.
        struct PackageCheckJob {
            ctx: Option<FolderAnalysisContext>,
            stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
            workspace: Option<ry_workspace::WorkspaceContext>,
            cache: Arc<Mutex<ProjectCache>>,
            files: Vec<PackageFile>,
        }
        let mut jobs: Vec<PackageCheckJob> = Vec::with_capacity(partitions.len());
        {
            let mut state = self.state.lock().await;
            for partition in partitions {
                let folder_root = partition.ctx.as_ref().map(|ctx| ctx.root.clone());
                let live = folder_root.as_ref().and_then(|folder_root| {
                    state
                        .folder_contexts
                        .iter_mut()
                        .find(|ctx| &ctx.root == folder_root)
                });
                match live {
                    Some(ctx) => {
                        let cache = ctx
                            .package_caches
                            .entry(partition.package_root.clone())
                            .or_default()
                            .clone();
                        // A package group with no stored context (its files
                        // arrived after the last index, e.g. a freshly
                        // created package) checks against an empty context
                        // until the next background index resolves it — the
                        // same staleness any new file already had, now
                        // scoped to its own package instead of inheriting a
                        // sibling package's metadata.
                        let workspace = ctx
                            .workspace_contexts
                            .get(&partition.package_root)
                            .cloned()
                            .unwrap_or_default();
                        jobs.push(PackageCheckJob {
                            ctx: partition.ctx,
                            stubs: Arc::clone(&ctx.stubs),
                            workspace: Some(workspace),
                            cache,
                            files: partition.files,
                        });
                    }
                    None if partition.ctx.is_none() => {
                        let cache = state
                            .root_caches
                            .entry(partition.package_root)
                            .or_default()
                            .clone();
                        jobs.push(PackageCheckJob {
                            ctx: partition.ctx,
                            stubs: Arc::clone(&state.user_stubs),
                            workspace: None,
                            cache,
                            files: partition.files,
                        });
                    }
                    // The owning folder vanished mid-pass (its removal also
                    // clears the affected URIs without advancing the
                    // debounce generation, which is what lets this pass run
                    // at all). Checking the orphaned partition through the
                    // root inputs would briefly republish diagnostics the
                    // removal just cleared, so skip it: the pass-end
                    // reconciliation settles the tracked URIs against
                    // current state.
                    None => continue,
                }
            }
        }

        let mut all_results: Vec<(Option<FolderAnalysisContext>, ProjectCheckResult)> = Vec::new();
        for job in jobs {
            let mut project = job.cache.lock().await;
            let result = project.check_with_workspace(job.files, job.stubs, job.workspace.as_ref());
            all_results.push((job.ctx, result));
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
    /// check. The bool answers the callers' original question — "did this
    /// pass settle the index duty it owed?" — so it is true for a
    /// wholesale install AND for a pass that settled without installing
    /// (an errored walk, or empty roots) while this pass's generation
    /// still held (the map is untouched but `initial_index_pending`
    /// must not strand); it is false when a newer scan or folder change
    /// supersedes the pass, leaving the republish to the supplanter.
    /// Callers that need to know whether fresh BYTES actually landed (the
    /// per-file refresh escalation) must use
    /// [`Backend::background_index_outcome`] and treat only
    /// [`BackgroundIndexOutcome::Installed`] as a landing.
    async fn spawn_background_index(&self) -> bool {
        matches!(
            self.background_index_outcome().await,
            BackgroundIndexOutcome::Installed | BackgroundIndexOutcome::SettledWithoutInstall
        )
    }

    /// The wholesale walk-and-commit pass behind
    /// [`Backend::spawn_background_index`], reporting which of the three
    /// ways it can end: see [`BackgroundIndexOutcome`].
    async fn background_index_outcome(&self) -> BackgroundIndexOutcome {
        // Test seam: count every attempted pass (test-util only), the
        // observable behind the paced driver's no-scan-per-retry
        // contract.
        #[cfg(feature = "test-util")]
        crate::test_seam::note_background_index_spawn();
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
                return BackgroundIndexOutcome::SettledWithoutInstall;
            }
            return BackgroundIndexOutcome::Superseded;
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
                // Per-package resolution contexts (#487): group the indexed
                // files by nearest-`DESCRIPTION` ancestor — the same
                // boundary `ry check` partitions on — and resolve each
                // group against its own package root, so one package's
                // `library()` union and NAMESPACE metadata never leak into
                // a sibling package checked in the same folder. The `None`
                // group (plain scripts outside any package) resolves
                // against the folder root, preserving today's folder-wide
                // visibility for it.
                let mut paths: Vec<&str> = outcome.files.keys().map(String::as_str).collect();
                paths.sort();
                let groups = ry_workspace::group_by_package_root(paths.iter().copied());
                let mut folder_contexts: HashMap<Option<PathBuf>, ry_workspace::WorkspaceContext> =
                    HashMap::with_capacity(groups.len());
                for (package_root, indices) in &groups {
                    let resolution_root = package_root.as_deref().unwrap_or(root);
                    // `paths` comes from this same map's keys, so the index
                    // cannot miss.
                    let files: Vec<&SourceFile> = indices
                        .iter()
                        .map(|index| outcome.files[paths[*index]].as_ref())
                        .collect();
                    match ry_workspace::resolve_workspace_context(
                        resolution_root,
                        config,
                        ry_workspace::ResolutionEnvironment {
                            files,
                            user_stubs: stubs,
                        },
                    ) {
                        Ok(context) => {
                            folder_contexts.insert(package_root.clone(), context);
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
                // Test seam: pause here (holding no lock) so a test can
                // land a per-file commit before this scan resumes (#526).
                #[cfg(feature = "test-util")]
                crate::test_seam::maybe_pause_scan_commit().await;
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
                drop(state);
                for (_, group) in &contexts {
                    for context in group.values() {
                        for (path, reason) in &context.degraded_scopes {
                            self.client.log_message(
                                tower_lsp::lsp_types::MessageType::WARNING,
                                format!("ry: {}: {reason}; using a file-stem binding. Raise max-serialized-bytes in ry.toml to enumerate it.", path.display()),
                            ).await;
                        }
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

    /// Schedule one debounced project publish pass covering `paths` for
    /// sessions with no open document (#528): a watched-file refresh that
    /// fixes a still-indexed, still-eligible closed file lands the right
    /// parse but never republishes — `republish_all_open_documents`
    /// degenerates to `clear_dropped_diagnostics`, which only clears
    /// paths that left the index. Each path joins `pending_diag_paths`
    /// through the existing `schedule_diagnostics` debounce, so the pass
    /// runs once per burst and never retriggers itself: the publish path
    /// only reads state and emits notifications, and emitting never
    /// re-enqueues. A no-op when an open document is present — that
    /// document's own scheduling already drives the pass — and when no
    /// path is tracked anymore.
    async fn schedule_closed_file_publish(&self, paths: Vec<String>) {
        let has_open_docs = { !self.state.lock().await.docs.is_empty() };
        if has_open_docs {
            return;
        }
        for path in paths {
            self.schedule_diagnostics(path_to_uri(&path)).await;
        }
    }

    /// Ensure the reconciliation driver runs when obligations are
    /// pending (P1). Cheap and idempotent — the flag check and the
    /// enqueue-side bookkeeping share the state lock, so calling this
    /// from every handler epilogue and every retained refresh exit
    /// cannot spawn more than one driver; see
    /// [`State::reconcile_driver_active`] for the lost-wakeup
    /// freedom argument. A no-op once shutdown began: the obligations
    /// are cancelled and no claim can re-seed them.
    async fn wake_reconciliation(&self) {
        let spawn_now = {
            let mut state = self.state.lock().await;
            if state.reconcile_driver_active
                || state.shutting_down
                || state.pending_refreshes.is_empty()
            {
                false
            } else {
                state.reconcile_driver_active = true;
                true
            }
        };
        if !spawn_now {
            return;
        }
        #[cfg(feature = "test-util")]
        crate::test_seam::note_reconciliation_driver_spawn();
        spawn_reconciliation_driver(self.clone());
    }

    /// Drive retained watched/close obligations until they settle —
    /// the no-rescue-event half of the convergence invariant. Each
    /// round snapshots the pending paths and re-runs, per path, exactly
    /// the pipeline the watched handler runs for a landed refresh: the
    /// per-file refresh ladder (which escalates to a full scan only
    /// after two lost generation races, so a quiet round is one read,
    /// not a rescan), the initial-index respawn duty, the package
    /// context refresh, and the publication. The phases keep their
    /// order because publishing before the context settles would show
    /// an analysis over a stale import context — bytes installed but
    /// obsolete context is not "settled" either.
    ///
    /// Liveness: every delivered event is finite, and once the burst
    /// stops nothing else moves the index generation, so the next
    /// round's refresh lands and drains its obligation; the loop exits
    /// through the idle transition (empty or cancelled map, flag
    /// cleared, both under the lock) the moment nothing is owed.
    /// Pathology is PACED, not dropped: after
    /// [`RECONCILE_ROUNDS_BEFORE_PACING`] rounds that retire nothing,
    /// the driver retains every obligation, warns once per stall
    /// episode, yields for a short capped backoff
    /// ([`reconcile_pace_delay`]), and retries — the bound limits the
    /// work performed per unit time (rounds per wall-clock, and never
    /// a fresh scan per retry beyond what a round's own ladder
    /// escalates), never the lifetime of the obligation, because a
    /// finite stall streak is not evidence that completion became
    /// impossible. The map empties only by settling, shutdown, or
    /// removal of the owning workspace. A round retires an obligation
    /// when one settles (removed from the map), so mid-round arrivals
    /// — which offset retirements in a map-length comparison while
    /// retiring nothing themselves — cannot masquerade as stagnation.
    /// Root removal and shutdown cancel obligations outright, and both
    /// the round head and every in-round dispatch re-check the
    /// shutdown flag under the state lock, so the driver never
    /// resurrects work or publications for a workspace that no longer
    /// exists.
    async fn run_reconciliation(&self) {
        loop {
            let (round, retired_before) = {
                let mut state = self.state.lock().await;
                if state.pending_refreshes.is_empty() || state.shutting_down {
                    state.reconcile_driver_active = false;
                    state.reconcile_rounds_without_progress = 0;
                    return;
                }
                (
                    state.pending_refreshes.keys().cloned().collect::<Vec<_>>(),
                    state.retired_refreshes,
                )
            };
            #[cfg(feature = "test-util")]
            crate::test_seam::note_reconciliation_round();
            // A pending initial index would make every refresh lose its
            // generation race to the completing pass and gate the
            // publications below; settle that duty first, exactly like
            // the watched handler's respawn step.
            if self.state.lock().await.initial_index_pending {
                self.spawn_background_index().await;
            }
            let mut follow_up: Vec<(String, u64)> = Vec::new();
            let mut did_work = false;
            for path in round {
                let duty = {
                    let state = self.state.lock().await;
                    // Shutdown cancels every obligation outright; a
                    // round already in progress re-checks here, under
                    // the lock, so no dispatch follows the cancelled
                    // map even when the purge landed mid-round.
                    if state.shutting_down {
                        None
                    } else {
                        state
                            .pending_refreshes
                            .get(&path)
                            // Never preempt an in-flight refresh: its read
                            // may still land under its own (newer-than-any
                            // driver) claim, and a driver-side re-claim would
                            // supersede it needlessly. Its exit re-wakes the
                            // driver if the obligation survives it.
                            .filter(|_| !state.refreshes_in_flight.contains_key(&path))
                            // The entry's epoch rides along as the
                            // completion token for the context-only
                            // arm: the bytes phase was settled by a
                            // landing that owed exactly this revision,
                            // and this round's follow-up may retire
                            // the entry only against that snapshot —
                            // taken at drive-time under this lock, not
                            // re-read at acknowledgement time, so a
                            // newer event re-arming the entry mid-round
                            // keeps its own fresh obligation.
                            .map(|pending| (pending.duty, pending.epoch))
                    }
                };
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
                crate::test_seam::maybe_pause_driver_idle().await;
                // Every surviving obligation is owned by an in-flight
                // refresh (or vanished mid-round) — but the duty checks
                // above ran under earlier, separate lock holds, and an
                // in-flight refresh may have exited in between with its
                // dispatcher's epilogue wake finding the flag still set
                // (waking nobody). Re-derive the idle verdict under the
                // SAME lock hold that clears the flag: if anything is
                // drivable now, keep driving instead of dropping the
                // signal; otherwise idle out — the exit-side wakeup
                // re-spawns a driver when an entry is idle and still
                // owed.
                let mut state = self.state.lock().await;
                if state.has_drivable_obligation() {
                    continue;
                }
                state.reconcile_driver_active = false;
                state.reconcile_rounds_without_progress = 0;
                return;
            }
            if !follow_up.is_empty() {
                // Shutdown may have landed while the dispatched
                // refreshes were running: their landings are already
                // committed index state, but the follow-up publications
                // belong to the ended session and must not go out. The
                // round head performs the idle transition on the next
                // pass.
                if self.state.lock().await.shutting_down {
                    continue;
                }
                // Same order as the watched handler: respawn a retired
                // initial pass, advance the contexts, then publish.
                let index_pending = { self.state.lock().await.initial_index_pending };
                if index_pending {
                    self.spawn_background_index().await;
                }
                let ctx_settled = self.refresh_package_contexts(&follow_up).await;
                self.publish_landed_paths(&follow_up).await;
                // Test seam: park between the context/publication
                // settlement and the obligation acknowledgement (see
                // `test_seam`).
                #[cfg(feature = "test-util")]
                crate::test_seam::maybe_pause_publication_ack().await;
                let mut state = self.state.lock().await;
                for (path, epoch) in &ctx_settled {
                    state.complete_pending_publication(path, *epoch);
                }
            }
            let paced = {
                let mut state = self.state.lock().await;
                match state.reconcile_round_progress(retired_before) {
                    // Everything is RETAINED; the driver yields for the
                    // computed backoff and retries. The retained paths
                    // ride along only on the round that ENTERS the stall
                    // episode, so the visible warning fires once per
                    // episode.
                    ReconcileRoundProgress::Pacing { delay, warn } => Some((
                        warn.then(|| state.pending_refreshes.keys().cloned().collect::<Vec<_>>()),
                        delay,
                    )),
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

    /// Schedule the debounced project publication for the paths a
    /// reconciliation round (or a handler follow-up) owes: with an open
    /// document anywhere, that document's own scheduling drives the
    /// project-wide pass — the pass publishes every checked file, not
    /// just open ones — mirroring `did_close`'s pattern; with none, the
    /// closed-file path carries them (#528). Takes the round's
    /// `(path, epoch)` pairs so the churn path pays no second
    /// path-vector copy; the epochs ride along unused here.
    async fn publish_landed_paths(&self, paths: &[(String, u64)]) {
        let open = {
            let state = self.state.lock().await;
            state.docs.keys().next().cloned()
        };
        match open {
            Some(path) => self.schedule_diagnostics(path_to_uri(&path)).await,
            None => {
                self.schedule_closed_file_publish(
                    paths.iter().map(|(path, _)| path.clone()).collect(),
                )
                .await
            }
        }
    }

    /// Refresh one `disk_files` entry from disk, applying the owning
    /// folder's eligibility and the walk's per-file admission rules with
    /// the same bounded decoder the background indexer parses through
    /// (#486). A missing, ineligible, oversized, or unreadable file is
    /// dropped from the index, so a watched-file deletion corrupts
    /// nothing and a rescan cannot disagree about membership. Returns
    /// the refresh's CLAIM EPOCH when it LANDED and `None` otherwise:
    /// the epoch is the completion token the caller's follow-up must
    /// acknowledge the obligation with (`complete_pending_publication`)
    /// — it came from the claim snapshotted before this call's read,
    /// so it names the revision whose bytes, context, and publication
    /// the follow-up demonstrably covered, never whatever entry happens
    /// to be current at acknowledgement time. Retries keep the original
    /// claim, so one call reports one epoch. A landed refresh claims
    /// the next `index_generation` atomically with its map write inside
    /// the commit critical section — the check, the write, and the
    /// bump share one lock hold — so no scan can commit between the
    /// insert and the retirement, and the caller must not bump again
    /// (#526). Landing also retires any in-flight background pass (its
    /// commit check fails), which is why the caller respawns the
    /// initial pass when the landed refresh retired it. The commit
    /// lands only when BOTH of the refresh's snapshots are still
    /// current: the snapshot generation (a newer scan, folder change,
    /// or landed refresh owns the fresher bytes or the fresher map, so
    /// a delayed commit must not install older source over them) and
    /// the per-path refresh epoch claimed at start, before the blocking
    /// read — watched-file handlers dispatch concurrently, so two
    /// refreshes for one path can snapshot the same generation, and
    /// without the epoch the older read would win whenever it commits
    /// first: it lands stale bytes, bumps the generation, and the newer
    /// read's commit then fails the generation check (#538). With the
    /// epoch, only the most recently started refresh for a path can
    /// commit, whichever commit reaches the lock first. The `did_open`
    /// guard below is a separate authority rule, not a freshness check:
    /// an open buffer shadows its disk twin regardless of generation. A
    /// refresh that loses the EPOCH race returns `None` — a newer
    /// same-path refresh owns the entry — so the caller neither
    /// republishes nor respawns scans for it. A refresh that loses the
    /// GENERATION race is different: a per-file refresh carries only
    /// its own path, and watched-file handlers dispatch concurrently,
    /// so the generation can move on an unrelated refresh (a close-time
    /// re-read, another path's event) whose landing says nothing about
    /// THIS path's bytes. Dropping the update outright would strand the
    /// watched event — with no open document nothing else republishes
    /// the path, so its fix or creation never reaches the client (#551)
    /// — so the refresh retries once from current state, and a second
    /// loss falls back to a full background scan, the same ladder
    /// [`Backend::refresh_one_package_context`] uses for the
    /// resolution maps. The retry re-reads from current disk, so its
    /// bytes postdate every writer that beat the previous attempt and
    /// installing them is always safe. The returned verdict is terminal
    /// for this event, never a retry signal: `None` means someone else
    /// owns the publication (a newer same-path refresh, an open buffer
    /// shadowing the disk bytes, a cap refusal, a backstop that did not
    /// land) — the retry ladder and the backstop live INSIDE this
    /// function, so a caller-side retry on `None` could only loop
    /// against a persistent owner.
    async fn refresh_disk_entry(&self, path: PathBuf) -> Option<u64> {
        let path_string = path.to_string_lossy().into_owned();
        // Claim the path's refresh epoch once, before the first
        // snapshot and read: the claim orders same-path refreshes by
        // START, and the commit below refuses any refresh a newer one
        // superseded, whichever commit lines up on the state lock
        // first (#538). Retries keep the original claim — re-claiming
        // would leapfrog a newer same-path refresh that started during
        // this call, inverting the start-order rule. The entry is
        // reclaimed only on commit arms that already passed the epoch
        // check (landed removal, cap refusal) — there the reclaimer
        // provably holds the path's LATEST claim, so nothing newer is
        // in flight to clobber. The early returns below the read
        // (open-document guard, generation-race exhaustion) run
        // before/outside the epoch check and deliberately keep the
        // claim: removing it there could delete a newer in-flight
        // refresh's entry and wrongly discard the freshest bytes, so
        // those slots persist until the path's next claim or a landed
        // removal — one per distinct refreshed path, never per event.
        // The claim also enqueues the reconciliation obligation BEFORE
        // the read that might lose its races (P1): the entry rides the
        // same claim, so a newer event's claim overwrites it with a
        // fresher full duty and this refresh can only ever settle the
        // revision it covers. Without the entry, a ladder that loses
        // both generation races AND its backstop scan returned while
        // forgetting the path — the watched event's whole duty
        // stranded with no open document and no future event to
        // rescue it. The in-flight slot pairs with the exit-side
        // decrement in the wrapper below, so the driver can tell a
        // path waiting for work from one whose refresh is merely
        // still running. A shutting-down session claims nothing: the
        // caller's whole follow-up (context, publication) belongs to
        // the ended session.
        let refresh_epoch = self.state.lock().await.claim_refresh_epoch(&path_string)?;
        let landed = self.refresh_disk_entry_claimed(path, refresh_epoch).await;
        let retained_idle = {
            let mut state = self.state.lock().await;
            let driver_may_take_over = state.note_refresh_exit(&path_string);
            // The exit's signal is split by what survived it: a landed
            // refresh leaves the context-and-publication phase to its
            // DISPATCHER (this call's caller), whose own follow-up and
            // epilogue wake cover it — waking here as well would spawn
            // a driver on every unopposed save, the pure overhead the
            // coalescing contract forbids. A refresh whose bytes never
            // landed (superseded, exhausted ladder, lost backstop)
            // leaves an obligation no one else is guaranteed to drive
            // except the caller's epilogue, so keep the driver
            // scheduled from the exit itself instead of trusting every
            // future caller to remember the epilogue wake.
            driver_may_take_over
                && state
                    .pending_refreshes
                    .get(&path_string)
                    .is_some_and(|pending| pending.duty == RefreshDuty::BytesContextPublication)
        };
        if retained_idle {
            self.wake_reconciliation().await;
        }
        // The claim epoch is the completion token: the caller's context
        // refresh and publication follow-up retire the obligation only
        // against the revision this refresh demonstrably covered.
        landed.then_some(refresh_epoch)
    }

    /// The ladder proper, running under a claim `refresh_disk_entry`
    /// already took; see there for the claim/exit bookkeeping.
    async fn refresh_disk_entry_claimed(&self, path: PathBuf, refresh_epoch: u64) -> bool {
        let path_string = path.to_string_lossy().into_owned();
        // Snapshot the owning root and config, plus the index
        // generation this attempt's commit must still hold when it
        // lands: the admission checks below must agree with each other
        // even if a config reload lands mid-refresh (a rescan converges
        // anything left over), and the commit must not overwrite a
        // newer scan's bytes or survive a newer same-path refresh (see
        // the two commit checks at the write).
        for attempt in 0..2 {
            let (
                walk_root,
                exclude_anchor,
                excludes,
                include_build_ignored,
                limits,
                check_test_fixtures,
                eligible,
                is_open,
                superseded,
                refresh_gen,
            ) = {
                let state = self.state.lock().await;
                let (
                    walk_root,
                    exclude_anchor,
                    excludes,
                    include_build_ignored,
                    limits,
                    check_test_fixtures,
                ) = match state.folder_context_for_path(&path_string) {
                    Some(ctx) => (
                        ctx.root.clone(),
                        ctx.config_root.clone().unwrap_or_else(|| ctx.root.clone()),
                        ctx.excludes.clone(),
                        ctx.config.include_build_ignored.clone(),
                        ry_workspace::DiscoveryLimits::from_config(&ctx.config),
                        ctx.config.check_test_fixtures,
                    ),
                    None => (
                        state.root.clone().unwrap_or_default(),
                        state
                            .root_config_dir
                            .clone()
                            .or_else(|| state.root.clone())
                            .unwrap_or_default(),
                        state.root_excludes.clone(),
                        state.file_config.include_build_ignored.clone(),
                        ry_workspace::DiscoveryLimits::from_config(&state.file_config),
                        state.file_config.check_test_fixtures,
                    ),
                };
                let eligible = state.eligibility_for_path(&path_string);
                let is_open = state.docs.contains_key(&path_string);
                let superseded = state.refresh_epochs.get(&path_string) != Some(&refresh_epoch);
                let refresh_gen = state.index_generation;
                (
                    walk_root,
                    exclude_anchor,
                    excludes,
                    include_build_ignored,
                    limits,
                    check_test_fixtures,
                    eligible,
                    is_open,
                    superseded,
                    refresh_gen,
                )
            };
            if is_open {
                // The editor's buffer is authoritative; the watched event (or
                // a save whose bytes the buffer already shadows) changes
                // nothing the publish path reads. Terminal for the
                // obligation: the buffer owns the path's analysis, and a
                // future didClose re-enqueues through its own close-time
                // re-read.
                let mut state = self.state.lock().await;
                state.settle_pending_refresh(&path_string, refresh_epoch, false);
                return false;
            }
            // A newer same-path refresh already claimed the epoch before
            // this attempt snapshotted — either it claimed inside the
            // claim-to-snapshot window of attempt 0, or a retry
            // re-snapshots after a lost generation race and a newer
            // refresh started meanwhile. The commit-time epoch check
            // below would refuse this attempt anyway; skipping its
            // blocking read now avoids a parse the verdict discards.
            // The authoritative check stays at the commit (the epoch can
            // still move during the read), and this early exit keeps the
            // claim, per the reclamation policy above.
            if superseded {
                tracing::debug!(
                    path = %path_string,
                    epoch = refresh_epoch,
                    "discarding superseded per-file disk refresh before its read"
                );
                return false;
            }
            // Clone the owning root for the commit-time budget check below:
            // `walk_root` moves into the blocking closure, and only the budget
            // check needs it afterwards — one small allocation per watched
            // event, not worth an `Arc` rippling through every root comparison.
            // `path` moves too; a retry re-reads through its own clone.
            let budget_root = walk_root.clone();
            let read_path = path.clone();
            let parsed = tokio::task::spawn_blocking(move || {
                if !eligible
                    || !single_file_admitted(
                        &read_path,
                        &walk_root,
                        Some(&exclude_anchor),
                        &excludes,
                        &include_build_ignored,
                        &limits,
                        check_test_fixtures,
                    )
                {
                    return None;
                }
                let decoded = ry_workspace::read_r_source_decoded(&read_path).ok()?;
                let path_string = read_path.to_string_lossy().into_owned();
                let mut parser = RParser::new().ok()?;
                let mut file = parser.parse(&path_string, &decoded.text).ok()?;
                decoded.attach_boundary_findings(&mut file);
                Some((path_string, Arc::new(file)))
            })
            .await
            .ok()
            .flatten();
            // Test seam: pause here (holding no lock) so a test can land a
            // newer writer before this commit resumes (#526).
            #[cfg(feature = "test-util")]
            crate::test_seam::maybe_pause_refresh_commit().await;
            let mut state = self.state.lock().await;
            // A concurrent `did_open` landed while the read was in flight:
            // installing a disk snapshot now would shadow the live buffer's
            // entry on the next publish assembly. Terminal for the same
            // reason as the snapshot-time guard above.
            if state.docs.contains_key(&path_string) {
                state.settle_pending_refresh(&path_string, refresh_epoch, false);
                return false;
            }
            // A newer index generation started while the blocking read was
            // in flight (a full scan, a folder change, or a landed refresh
            // that retired in-flight writers): the newer writer owns the
            // fresher bytes or the fresher map, and this commit's older
            // parse must not install over them (#526). But a per-file
            // refresh carries only its own path, and the generation may
            // have moved on an UNRELATED refresh whose landing says
            // nothing about this path — dropping the update outright
            // would strand the watched event, because with no open
            // document nothing else republishes the path (#551). Retry
            // once from current state instead: the re-read postdates
            // every writer that beat this attempt, so installing the
            // retry's bytes is always safe. Drop the lock first — the
            // retry re-snapshots below.
            if state.index_generation != refresh_gen {
                drop(state);
                if attempt == 0 {
                    tracing::debug!(
                        path = %path_string,
                        gen = refresh_gen,
                        "retrying per-file disk refresh after a lost generation race"
                    );
                    continue;
                }
                tracing::warn!(
                    path = %path_string,
                    gen = refresh_gen,
                    "per-file disk refresh lost two generation races; converging through a full scan"
                );
                break;
            }
            // A newer refresh for this same path started while this one was
            // in flight: the generation check above cannot order two
            // same-path refreshes that snapshot one generation — the older
            // read, committing first, would land its bytes and bump the
            // generation, making the NEWER read's commit look stale and
            // leaving the older contents indexed (#538). The start-time
            // epoch claim breaks the tie: only the most recently started
            // refresh for a path can commit, so last-write-wins is decided
            // by read order, not commit order. A superseded refresh bumps
            // nothing — the newer one still owns the entry and the
            // generation.
            if state.refresh_epochs.get(&path_string) != Some(&refresh_epoch) {
                tracing::debug!(
                    path = %path_string,
                    epoch = refresh_epoch,
                    "discarding superseded per-file disk refresh"
                );
                return false;
            }
            match parsed {
                Some((parsed_path, file)) => {
                    // The `index.max-files` count is not a path property, so the
                    // admission verdict above cannot enforce it: refuse a new
                    // entry once the owning root is at its budget (#525).
                    // First-come-first-served like the walk (though the orders
                    // differ — readdir vs. event arrival — so above the cap the
                    // retained subset may diverge from a fresh scan's):
                    // refreshing an already-indexed path still lands (no
                    // growth), and nothing already indexed is evicted — so the
                    // incremental map never holds more per root than a fresh
                    // scan of the same tree.
                    // Decided here under the lock, not in the blocking snapshot:
                    // two concurrent refreshes racing at the cap line up on this
                    // lock, and only the first one through grows the map.
                    let over_budget = !budget_root.as_os_str().is_empty()
                        && !state.disk_files.contains_key(&parsed_path)
                        && state
                            .disk_files
                            .keys()
                            .filter(|existing| {
                                state.entry_consumes_root_budget(
                                    std::path::Path::new(existing.as_str()),
                                    &budget_root,
                                )
                            })
                            .count()
                            >= limits.max_files;
                    if over_budget {
                        tracing::warn!(
                            path = %parsed_path,
                            root = %budget_root.display(),
                            cap = limits.max_files,
                            "discarding per-file disk refresh over index.max-files"
                        );
                        // The refusal passed the epoch check, so this refresh
                        // is the path's latest and nothing newer is in
                        // flight: reclaim the epoch entry here too — a path
                        // the cap keeps refusing never enters the index, so
                        // its claim would otherwise be the one entry nothing
                        // else retires (#538) — and settle the obligation
                        // (terminal refusal: the path deliberately stays
                        // unindexed, like a fresh scan of the same tree
                        // would leave it).
                        state.refresh_epochs.remove(&parsed_path);
                        state.settle_pending_refresh(&parsed_path, refresh_epoch, false);
                        return false;
                    }
                    // Claim the next generation in the same critical section
                    // as the insert: the check above, the write, and the bump
                    // share one lock hold, so no scan can commit between the
                    // insert and the retirement — the staleness check no later
                    // writer can slip through. The caller must not bump again;
                    // the landed `true` already carries the retirement (#526).
                    // Landed: settle the bytes phase of the obligation in
                    // the same critical section as the insert, under the
                    // epoch check that just passed — the caller's context
                    // refresh and publication follow-up owe the rest.
                    state.settle_pending_refresh(&parsed_path, refresh_epoch, true);
                    state.disk_files.insert(parsed_path, file);
                    state.index_generation = state.index_generation.wrapping_add(1);
                    drop(state);
                    #[cfg(feature = "test-util")]
                    crate::test_seam::maybe_pause_post_refresh_commit().await;
                    return true;
                }
                None => {
                    // Unreadable, unparseable, or walk-inadmissible: match the
                    // walk, which never lands such a file in the map. A
                    // dropped entry that previously carried diagnostics is
                    // reconciled by the caller's republish pass (#489).
                    // The removal lands like an insert — same atomic
                    // retirement, same caller contract.
                    state.settle_pending_refresh(&path_string, refresh_epoch, true);
                    state.disk_files.remove(&path_string);
                    // The remover held the path's latest epoch (the check
                    // above), so no same-path refresh is in flight behind
                    // it: the epoch entry leaves with the index entry it
                    // ordered, and the map holds at most one entry per
                    // path the session has refreshed — never one per event
                    // (#538). The full scan's wholesale install deliberately
                    // does NOT prune the epoch map against its new key set:
                    // a refresh that started after the scan's generation
                    // bump may still hold genuinely newer bytes for a path
                    // the scan's walk missed, and deleting its claim would
                    // wrongly discard them. A later refresh re-seeds from
                    // the global counter, so a reclaimed slot cannot alias
                    // a live claim short of the u64 wrap the index
                    // generation already accepts.
                    state.refresh_epochs.remove(&path_string);
                    state.index_generation = state.index_generation.wrapping_add(1);
                    drop(state);
                    #[cfg(feature = "test-util")]
                    crate::test_seam::maybe_pause_post_refresh_commit().await;
                    return true;
                }
            }
        } // retry loop
        // Two lost generation races in a row — a commit landed inside
        // each of two consecutive snapshot-to-commit windows — so the
        // session is under sustained index churn. Converge the whole
        // index through the generation-guarded full scan, the same
        // backstop `refresh_one_package_context` uses for the
        // resolution maps: the scan re-reads every path from current
        // disk and installs only when it is the newest writer. The
        // scan's verdict is threaded through: a LANDED scan installed
        // this path's current disk bytes, so returning true hands the
        // caller the publication duty — the watched handler pushes the
        // path onto its landed list and `schedule_closed_file_publish`
        // drives the republish, the #528 convention every other
        // no-open-document scan call site already follows (the callers'
        // `refresh_package_contexts` is redundant-but-harmless after
        // the scan's wholesale context rebuild). A superseded or
        // settled-without-install scan returns false and the path's
        // obligation stays RETAINED for the reconciliation driver (P1):
        // "converges on its own next event" was the residual the
        // no-rescue-event invariant forbids — with no open document and
        // silence, nothing else would ever re-drive the path. Only the
        // landed arm carries the published-immediately guarantee. The
        // churn bound
        // is the double-loss precondition itself — two commits inside
        // two consecutive sub-millisecond snapshot-to-commit windows per
        // escalated refresh — plus each spawned pass stays
        // generation-guarded, so superseded walks discard without
        // writing. Only [`BackgroundIndexOutcome::Installed`] counts as
        // this path's landing: the scan's bool-compatible contract is
        // broader (a pass that settled without installing — an errored
        // walk, or empty roots, generation still held — also returns
        // true there, because it settles `initial_index_pending`
        // while leaving the map untouched), and treating that as a
        // landing would republish stale bytes under a success verdict.
        // A settled-without-install or superseded backstop leaves the
        // path to converge on its own next event, like the superseded
        // arm above.
        let landed = matches!(
            self.background_index_outcome().await,
            BackgroundIndexOutcome::Installed
        );
        if landed {
            // The scan fence-covers this claim: the walk re-read the
            // path after the scan started (which is after the claim),
            // and the write the event carries predates the claim, so
            // the installed bytes include it — and the scan's wholesale
            // commit rebuilt every package context in the same critical
            // section. Only the caller's publication follow-up remains
            // owed. A superseded or settled-without-install scan
            // settles nothing: the obligation stays retained, and the
            // reconciliation driver re-drives the path.
            let mut state = self.state.lock().await;
            state.settle_pending_refresh(&path_string, refresh_epoch, true);
        }
        landed
    }

    /// Re-resolve the owning package group's resolution context after
    /// per-file disk refreshes landed (#527). A refreshed file's parse
    /// reaches `disk_files` but its per-file resolution entries (configured
    /// globals, `library()` attachments, imports, load bindings) are
    /// otherwise rebuilt only by a full background scan, so a watched
    /// addition checks against an empty context until the next scan. Each
    /// affected group re-runs the same `resolve_workspace_context` pass
    /// the scan uses, over the current disk index scoped to the owning
    /// folder, and installs the result when no newer writer landed
    /// meanwhile. Disk-only inputs mirror the scan exactly (open buffers
    /// keep shadowing at publish time), so an incremental install can
    /// never disagree with a fresh scan over the same tree.
    ///
    /// Convergence: the install is generation-guarded like every other
    /// index writer (#526). On a lost race a single retry re-snapshots
    /// from current state; a second loss falls back to a full background
    /// scan, which resolves every group. Group-keyed installs commute
    /// (disjoint per-file keys), and same-path concurrent refreshes
    /// stay ordered by the per-path refresh epoch (#538), so this
    /// adds no interleave left to untangle: only landed refreshes
    /// reach this function, and the landing refresh for a path is
    /// always its most recently started one.
    ///
    /// The `(path, epoch)` pairs carry each landing's completion token
    /// through the context phase, and the returned settled pairs keep
    /// it, so the caller's acknowledgement retires only the revision
    /// the completed context-and-publication work demonstrably covered
    /// (see [`State::complete_pending_publication`]).
    async fn refresh_package_contexts(&self, landed: &[(String, u64)]) -> Vec<(String, u64)> {
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
                    // Root-owned partitions check against no stored
                    // context (the publish path passes `None`), so there
                    // is nothing to advance for them.
                    None => {
                        settled.push((path.clone(), *epoch));
                        continue;
                    }
                };
                let package_root = ry_workspace::enclosing_package_root(std::path::Path::new(path));
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
            // Snapshot the folder's owned (path, file) pairs plus config,
            // stubs, and the generation the install must still hold. Open
            // buffers stay out: the scan resolves disk state only, and
            // the publish path layers open documents over it afterwards.
            // Only clones move under the lock — the package-root grouping
            // below probes the filesystem (ancestor DESCRIPTION stats per
            // distinct directory) and runs inside the blocking closure,
            // like the scan's own grouping.
            struct Snapshot {
                candidates: Vec<(String, Arc<SourceFile>)>,
                config: ry_config::Config,
                stubs: Arc<std::collections::BTreeMap<String, ry_typeshed::Typeshed>>,
                resolution_root: PathBuf,
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
                    // The owning folder vanished mid-refresh; the folder
                    // removal path already cleared and republished, so
                    // nothing remains owed for this group.
                    None => return true,
                };
                // Component-based membership: a string prefix breaks
                // when the folder is the filesystem root (`//` never
                // prefixes `/project/main.R`).
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
                    resolution_root: package_root
                        .clone()
                        .unwrap_or_else(|| folder_root.to_path_buf()),
                    generation: state.index_generation,
                }
            };
            let package_root_clone = package_root.clone();
            let resolved = tokio::task::spawn_blocking(move || {
                let mut files: Vec<&SourceFile> = Vec::new();
                let mut root_cache: HashMap<Option<&std::path::Path>, Option<PathBuf>> =
                    HashMap::new();
                for (path, file) in &snapshot.candidates {
                    let fs_path = std::path::Path::new(path);
                    let key = fs_path.parent();
                    let root = root_cache
                        .entry(key)
                        .or_insert_with(|| ry_workspace::enclosing_package_root(fs_path))
                        .clone();
                    if root == package_root_clone {
                        files.push(file.as_ref());
                    }
                }
                ry_workspace::resolve_workspace_context(
                    &snapshot.resolution_root,
                    &snapshot.config,
                    ry_workspace::ResolutionEnvironment {
                        files,
                        user_stubs: &snapshot.stubs,
                    },
                )
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
            crate::test_seam::maybe_pause_context_install().await;
            // Installed: degraded-scope warnings keep scan parity (the
            // scan logs one line per scope per generation). Cloned before
            // the insert moves the context; never re-read — a concurrent
            // replacement's scopes are that writer's duty to log.
            let degraded = context.degraded_scopes.clone();
            {
                let mut state = self.state.lock().await;
                if state.index_generation != snapshot.generation {
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
            }
            for (path, reason) in &degraded {
                self.client
                    .log_message(
                        tower_lsp::lsp_types::MessageType::WARNING,
                        format!("ry: {}: {reason}; using a file-stem binding. Raise max-serialized-bytes in ry.toml to enumerate it.", path.display()),
                    )
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

/// Whether one on-disk R file would survive the directory walk's admission
/// rules, through the shared [`ry_workspace::is_single_file_walk_admitted`]
/// verdict: the extension set, symlink handling, pruned ancestor shapes,
/// testthat runner-code classification, the shared per-file eligibility
/// policy (excludes plus the size and depth caps, so the caps cannot drift
/// from the walker's or the open-buffer gate's reading of them, #488),
/// ancestor `exclude` patterns, and `.Rbuildignore` with
/// `include_build_ignored` rescue (#524). Runs on the blocking pool next to
/// the read it guards. Deliberately narrower than a full walk in one
/// documented way: the `index.max-files` count is not a path property, so
/// the commit in [`Backend::refresh_disk_entry`] enforces it against live
/// entry accounting instead (#525).
fn single_file_admitted(
    path: &Path,
    walk_root: &Path,
    exclude_anchor: Option<&Path>,
    excludes: &ry_config::Excludes,
    include_build_ignored: &[String],
    limits: &ry_workspace::DiscoveryLimits,
    check_test_fixtures: bool,
) -> bool {
    ry_workspace::is_single_file_walk_admitted(
        path,
        walk_root,
        exclude_anchor,
        excludes,
        include_build_ignored,
        limits,
        check_test_fixtures,
    )
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
            workspace_contexts: HashMap::new(),
            baseline,
            filter,
            min_confidence,
            excludes,
            package_caches: HashMap::new(),
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
/// `workspace_contexts`, and `package_caches` are not config-file-derived
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
        workspace_contexts: old.workspace_contexts.clone(),
        baseline,
        filter,
        min_confidence,
        excludes,
        package_caches: old.package_caches.clone(),
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

#[cfg(test)]
mod reconcile_accounting_tests {
    use super::*;

    /// The driver's pacing bound must count OBLIGATIONS RETIRED, not a
    /// shrink of `pending_refreshes`: a round can genuinely retire an
    /// obligation while a mid-round event arrival re-seeds the map, so
    /// eight progressing rounds under sustained arrivals would still
    /// hit a length-based bound and pace against obligations that were
    /// being serviced. Drive the accounting directly: eight offsetting
    /// rounds (one retirement, one arrival each — the map never
    /// shrinks) never engage pacing, and only truly retirement-free
    /// rounds pace — RETAINING everything, never dropping it.
    #[test]
    fn pacing_counts_retirements_not_map_shrinkage() {
        let mut state = State::default();
        let epoch = state.claim_refresh_epoch("/a.R").unwrap();
        assert!(state.note_refresh_exit("/a.R"));

        // Eight rounds that each retire one obligation while one
        // arrival lands mid-round: the map size is constant throughout,
        // so a length comparison would count every round as stalled.
        let mut current = "/a.R";
        for round in 0..8 {
            let retired_before = state.retired_refreshes;
            // The round drives `current` to a terminal outcome
            // (settling removes — retires — the obligation)...
            state.settle_pending_refresh(current, epoch.wrapping_add(round), false);
            // ...while an event arrives for a fresh path, claimed and
            // still in flight when the round ends.
            let arrival = format!("/arrival{round}.R");
            state.claim_refresh_epoch(&arrival).unwrap();
            assert_eq!(
                state.retired_refreshes,
                retired_before + 1,
                "round {round} must retire exactly its driven obligation"
            );
            assert!(
                matches!(
                    state.reconcile_round_progress(retired_before),
                    ReconcileRoundProgress::Progress
                ),
                "a round that retired an obligation is progress; arrivals cannot mask it"
            );
            assert_eq!(
                state.reconcile_rounds_without_progress, 0,
                "retiring rounds must reset the stall count"
            );
            current = Box::leak(arrival.into_boxed_str());
        }

        // Retirement-free rounds over the survivor: the first seven
        // accumulate, the eighth ENTERS the stall episode (warning,
        // base delay) and every later one keeps pacing at a grown,
        // capped delay — but the obligation is retained throughout,
        // which is the whole contract: a finite stall streak must never
        // cancel the work it owes.
        for stalled in 1..=11u32 {
            let retired_before = state.retired_refreshes;
            let progress = state.reconcile_round_progress(retired_before);
            assert_eq!(state.retired_refreshes, retired_before);
            if stalled < RECONCILE_ROUNDS_BEFORE_PACING {
                assert!(
                    matches!(progress, ReconcileRoundProgress::Stalled),
                    "round {stalled} is below the bound"
                );
                assert_eq!(state.reconcile_rounds_without_progress, stalled);
            } else {
                let ReconcileRoundProgress::Pacing { delay, warn } = progress else {
                    panic!("round {stalled} must pace, not drop");
                };
                assert_eq!(delay, reconcile_pace_delay(stalled));
                assert_eq!(
                    warn,
                    stalled == RECONCILE_ROUNDS_BEFORE_PACING,
                    "the warning fires exactly once per stall episode"
                );
                assert!(
                    state.pending_refreshes.contains_key(current),
                    "a paced round retains the obligation"
                );
            }
        }
        assert_eq!(
            state.reconcile_rounds_without_progress, 11,
            "paced rounds keep accumulating until a retirement resets them"
        );

        // A retirement ends the episode: the next retirement-free
        // streak starts a fresh one that warns again at its own
        // crossing, from the base delay.
        state.settle_pending_refresh(current, u64::MAX, false);
        assert!(matches!(
            state.reconcile_round_progress(state.retired_refreshes.wrapping_sub(1)),
            ReconcileRoundProgress::Progress
        ));
        assert_eq!(state.reconcile_rounds_without_progress, 0);
        state.claim_refresh_epoch("/next.R").unwrap();
        for _ in 0..RECONCILE_ROUNDS_BEFORE_PACING - 1 {
            let _ = state.reconcile_round_progress(state.retired_refreshes);
        }
        let ReconcileRoundProgress::Pacing { delay, warn } =
            state.reconcile_round_progress(state.retired_refreshes)
        else {
            panic!("the fresh streak must reach the pacing bound");
        };
        assert!(warn, "a new stall episode warns again");
        assert_eq!(delay, reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING));
        assert!(state.pending_refreshes.contains_key("/next.R"));
    }

    /// The pace ladder doubles from the base and caps: the observable
    /// half of the no-spin contract (rounds per wall-clock are bounded
    /// by these rungs while completion stays impossible).
    #[test]
    fn pace_delay_ladder_is_bounded() {
        assert_eq!(
            reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS)
        );
        assert_eq!(
            reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING + 1),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS * 2)
        );
        assert_eq!(
            reconcile_pace_delay(RECONCILE_ROUNDS_BEFORE_PACING + 2),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS * 4)
        );
        for stalled in RECONCILE_ROUNDS_BEFORE_PACING..(RECONCILE_ROUNDS_BEFORE_PACING + 50) {
            assert!(
                reconcile_pace_delay(stalled)
                    <= std::time::Duration::from_millis(RECONCILE_PACE_CAP_MILLIS)
            );
        }
        // Below the bound the driver does not sleep at all; rung 0
        // still computes the base delay, pinning the ladder's floor.
        assert_eq!(
            reconcile_pace_delay(0),
            std::time::Duration::from_millis(RECONCILE_PACE_BASE_MILLIS)
        );
    }

    /// The completion token is version-specific: an older refresh's
    /// acknowledgement carries its own claim epoch, and a newer
    /// event's re-armed obligation — even after it advances to the
    /// context/publication phase, matching the older operation's
    /// completed phase by name — must survive that acknowledgement.
    /// Only the newer revision's own completion (or an operation that
    /// demonstrably covers its epoch) may retire it.
    #[test]
    fn older_completion_cannot_retire_a_newer_pending_entry() {
        let mut state = State::default();
        // Refresh A claims, exits, lands its bytes, and settles its
        // context — everything but the final acknowledgement.
        let epoch_a = state.claim_refresh_epoch("/a.R").unwrap();
        assert!(state.note_refresh_exit("/a.R"));
        state.settle_pending_refresh("/a.R", epoch_a, true);
        // Before A acknowledges, refresh B claims (re-arming the entry
        // at a higher epoch with a FULL duty) and lands ITS bytes: the
        // entry advances to the context/publication phase at B's
        // epoch — the exact shape a duty-only acknowledgement would
        // mistake for A's completed phase.
        let epoch_b = state.claim_refresh_epoch("/a.R").unwrap();
        assert!(epoch_b > epoch_a);
        assert!(state.note_refresh_exit("/a.R"));
        state.settle_pending_refresh("/a.R", epoch_b, true);

        let retired_before = state.retired_refreshes;
        state.complete_pending_publication("/a.R", epoch_a);
        assert_eq!(
            state.retired_refreshes, retired_before,
            "A's older acknowledgement must retire nothing"
        );
        let pending = state
            .pending_refreshes
            .get("/a.R")
            .expect("B's entry must survive A's acknowledgement");
        assert_eq!(pending.epoch, epoch_b);
        assert_eq!(pending.duty, RefreshDuty::ContextPublication);

        // B's own completion retires it.
        state.complete_pending_publication("/a.R", epoch_b);
        assert_eq!(state.retired_refreshes, retired_before + 1);
        assert!(!state.pending_refreshes.contains_key("/a.R"));
        // A late replay of the older acknowledgement is a no-op.
        state.complete_pending_publication("/a.R", epoch_a);
        assert_eq!(state.retired_refreshes, retired_before + 1);
    }

    /// The token also authorizes the one legitimate cross-version
    /// retirement: a NEWER operation's completion covers an older
    /// entry's remaining duty (it read and landed after the older
    /// claim), while a FULL duty re-armed by an even newer event is
    /// owed its whole pipeline and survives any older token.
    #[test]
    fn completion_token_covers_older_entries_but_not_full_duties() {
        let mut state = State::default();
        let epoch_a = state.claim_refresh_epoch("/a.R").unwrap();
        state.note_refresh_exit("/a.R");
        state.settle_pending_refresh("/a.R", epoch_a, true);
        // A newer refresh (epoch_c > epoch_a) completes the
        // context/publication phase without re-claiming in between: its
        // acknowledgement covers A's revision.
        let epoch_c = epoch_a + 7;
        state.complete_pending_publication("/a.R", epoch_c);
        assert!(!state.pending_refreshes.contains_key("/a.R"));
        assert_eq!(state.retired_refreshes, 1);

        // An event re-arms the entry with a full duty at a fresh
        // epoch; no acknowledgement, however new, may retire a FULL
        // duty by phase name alone.
        let epoch_d = state.claim_refresh_epoch("/a.R").unwrap();
        state.note_refresh_exit("/a.R");
        state.complete_pending_publication("/a.R", epoch_d + 100);
        let pending = state
            .pending_refreshes
            .get("/a.R")
            .expect("a full duty survives any publication acknowledgement");
        assert_eq!(pending.epoch, epoch_d);
        assert_eq!(pending.duty, RefreshDuty::BytesContextPublication);
        assert_eq!(state.retired_refreshes, 1);
    }

    /// In-flight accounting must be independent of the obligation map's
    /// removals: a cancellation (folder removal, shutdown) can drop a
    /// pending entry while its refresh is still in flight, and a later
    /// event re-enqueues a FRESH obligation for the same path. The old
    /// refresh's exit must not decrement the new obligation's counter
    /// — an undercount the driver would read as "idle" and preempt the
    /// in-flight read with.
    #[test]
    fn in_flight_accounting_survives_obligation_cancellation() {
        let mut state = State::default();
        assert!(state.claim_refresh_epoch("/a.R").is_some());
        // The cancellation drops the obligation (root-removal retain,
        // shutdown clear) but cannot recall the refresh.
        state.pending_refreshes.clear();
        // A later event re-enqueues before the old refresh returns.
        let rearm_epoch = state.claim_refresh_epoch("/a.R").unwrap();
        assert_eq!(
            state.refresh_epochs.get("/a.R"),
            Some(&rearm_epoch),
            "the fresh claim owns the path"
        );
        assert!(
            !state.note_refresh_exit("/a.R"),
            "the old refresh's exit must not report the new obligation drivable"
        );
        assert!(
            !state.has_drivable_obligation(),
            "the driver must not preempt the re-armed refresh that is still in flight"
        );
        assert!(
            state.note_refresh_exit("/a.R"),
            "the owning refresh's exit is what makes the obligation drivable"
        );
        assert!(state.has_drivable_obligation());
    }

    /// A shut-down session refuses every claim, so no post-shutdown
    /// event can re-seed the obligation map the shutdown purged.
    #[test]
    fn shutdown_refuses_refresh_claims() {
        let mut state = State::default();
        assert!(state.claim_refresh_epoch("/a.R").is_some());
        state.shutting_down = true;
        state.pending_refreshes.clear();
        assert!(state.claim_refresh_epoch("/b.R").is_none());
        assert!(state.pending_refreshes.is_empty());
        assert!(!state.has_drivable_obligation());
    }
}
