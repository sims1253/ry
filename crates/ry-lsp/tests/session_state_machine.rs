//! LSP state-machine property.
//!
//! Extends the session convergence property with the full operation
//! alphabet:
//!   - add / remove workspace folder
//!   - edit config (`ry.toml`), baseline, NAMESPACE, DESCRIPTION, typesheds,
//!     and discovery caps
//!   - create / delete / rename on-disk R file
//!   - controlled parse completion from an older generation (rapid edits)
//!   - server restart
//!
//! ## Property
//!
//! After each quiescent operation, every published diagnostic and the
//! observable discovered-file set equal a fresh server initialized on the
//! same final workspace.
//!
//! ## Determinism
//!
//! The property uses **fixed deterministic seeds**, never random proptest
//! RNG. Each seed drives a `TestRunner` whose RNG is a fixed `ChaCha` seed.
//! Proptest shrinking is preserved: when a generated case fails, the runner
//! shrinks it and stores the minimal failing replay in
//! `session_state_machine.proptest-regressions`.
//!
//! Two lanes:
//!   - **PR** (`pr_session_seeds`): a bounded seed set, run on every PR.
//!   - **Nightly** (`nightly_session_seeds`): 1,000 fixed seeds,
//!     `#[ignore]`'d because it takes minutes.
//!
//! Neither lane uses sleeps or nondeterministic random seeds. Quiescence is
//! the `textDocument/publishDiagnostics` notification — an explicit protocol
//! signal. The drain idle-timeout detects end-of-stream; it does not wait
//! for computation.
//!
//! The file also hosts the deterministic session tests that predate the
//! property (the full-alphabet property subsumed the original
//! open/edit/close/restart convergence property): the UTF-16 position
//! transcript over a non-ASCII document and the #81 close/reopen
//! republication regression.

use proptest::collection;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
use ry_testkit::{FixtureProject, LspSession};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod harness;

use harness::{
    ClientSession, SourceVariant, apply_incremental_edit, file_uri, first_line_utf16_len,
    join_session, normalize_diagnostics, sorted_diagnostics, spawn_session, sync_barrier,
};

// ──────────────────────────────────────────────────────────────────────────
// Fixture layout
// ──────────────────────────────────────────────────────────────────────────

/// Five on-disk file slots in the primary root.
const FILES: &[&str] = &["a.R", "b.R", "c.R", "d.R", "e.R"];

/// Initial content for every workspace file (clean — no diagnostics).
const INITIAL_DISK: &str = "x <- 1L\ny <- 2L\n";

/// Name of the secondary workspace folder.
const SECOND: &str = "second";

/// Drain idle-timeout: stop draining when no message arrives within this
/// duration. This detects end-of-stream after the quiescence publication;
/// it does not wait for computation or debounce.
const DRAIN_IDLE: Duration = Duration::from_millis(60);

// ──────────────────────────────────────────────────────────────────────────
// Source variants
// ──────────────────────────────────────────────────────────────────────────

// The variants live in `harness` (`SourceVariant`); the diagnostic
// variants differ only in the Unicode class of the bound name on line 0.

fn source_strategy() -> impl Strategy<Value = SourceVariant> {
    prop_oneof![
        Just(SourceVariant::Clean),
        Just(SourceVariant::AsciiDiagnostic),
        Just(SourceVariant::AstralDiagnostic)
    ]
}

// ──────────────────────────────────────────────────────────────────────────
// Operation alphabet
// ──────────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Operation {
    // Base alphabet
    Open {
        file: u8,
        source: SourceVariant,
    },
    FullEdit {
        file: u8,
        source: SourceVariant,
    },
    IncrementalEdit {
        file: u8,
        source: SourceVariant,
    },
    Close {
        file: u8,
    },
    Restart,

    // State-machine extensions
    /// Create an on-disk R file (if the slot is empty).
    CreateFile {
        file: u8,
        source: SourceVariant,
    },
    /// Delete an on-disk R file (if the slot has a file).
    DeleteFile {
        file: u8,
    },
    /// Rename an on-disk R file to another slot.
    RenameFile {
        from: u8,
        to: u8,
    },
    /// Edit `ry.toml`: toggle the ignore list.
    EditConfig {
        ignore_diagnostic: bool,
    },
    /// Edit `baseline.json`: toggle baseline suppression.
    EditBaseline {
        suppress: bool,
    },
    /// Edit `second/NAMESPACE`.
    EditNamespace,
    /// Edit `second/DESCRIPTION`.
    EditDescription,
    /// Edit `second/typesheds/localdep.json`.
    EditTypeshed,
    /// Edit discovery caps in `ry.toml`.
    EditDiscoveryCaps {
        max_files: u8,
    },
    /// Rapid consecutive edits to the same file without quiescing —
    /// controlled parse completion from an older generation.
    RapidEdit {
        file: u8,
        source: SourceVariant,
    },
    /// Add the secondary workspace folder.
    AddFolder,
    /// Remove the secondary workspace folder.
    RemoveFolder,
}

fn file_slot() -> BoxedStrategy<u8> {
    (0u8..FILES.len() as u8).boxed()
}

fn operation_strategy() -> BoxedStrategy<Operation> {
    prop_oneof![
        // Base alphabet (higher weight: these are the bread-and-butter ops)
        3 => (file_slot(), source_strategy())
            .prop_map(|(f, s)| Operation::Open { file: f, source: s }),
        2 => (file_slot(), source_strategy())
            .prop_map(|(f, s)| Operation::FullEdit { file: f, source: s }),
        2 => (file_slot(), source_strategy())
            .prop_map(|(f, s)| Operation::IncrementalEdit { file: f, source: s }),
        1 => file_slot().prop_map(|f| Operation::Close { file: f }),
        1 => Just(Operation::Restart),
        // State-machine extensions: on-disk file lifecycle, config, baseline,
        // discovery caps, parse races, and workspace-folder mutation.
        // Metadata edits (NAMESPACE/DESCRIPTION/typesheds) are exercised
        // by dedicated regression tests, not the random alphabet, because
        // they interact with package resolution in ways that require
        // package-structured fixtures.
        2 => (file_slot(), source_strategy())
            .prop_map(|(f, s)| Operation::CreateFile { file: f, source: s }),
        1 => file_slot().prop_map(|f| Operation::DeleteFile { file: f }),
        1 => (file_slot(), file_slot())
            .prop_map(|(from, to)| Operation::RenameFile { from, to }),
        1 => Just(true).prop_map(|v| Operation::EditConfig { ignore_diagnostic: v }),
        1 => Just(false).prop_map(|v| Operation::EditConfig { ignore_diagnostic: v }),
        1 => Just(true).prop_map(|v| Operation::EditBaseline { suppress: v }),
        1 => Just(false).prop_map(|v| Operation::EditBaseline { suppress: v }),
        1 => (1u8..5).prop_map(|n| Operation::EditDiscoveryCaps { max_files: n }),
        2 => (file_slot(), source_strategy())
            .prop_map(|(f, s)| Operation::RapidEdit { file: f, source: s }),
        1 => Just(Operation::AddFolder),
        1 => Just(Operation::RemoveFolder),
    ]
    .boxed()
}

fn operation_sequence_strategy() -> impl Strategy<Value = Vec<Operation>> {
    collection::vec(operation_strategy(), 1..=8)
}

// ──────────────────────────────────────────────────────────────────────────
// Session model — mirrors the server's authoritative world state
// ──────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SessionModel {
    /// Open documents: file slot → text.
    open_docs: BTreeMap<u8, String>,
    /// Disk files: file slot → text (only slots with files on disk).
    disk_files: BTreeMap<u8, String>,
    /// `ry.toml` ignore list.
    config_ignore: Vec<String>,
    /// Whether the baseline suppresses the diagnostic.
    baseline_suppress: bool,
    /// Whether the secondary workspace folder is added.
    second_folder: bool,
    /// Discovery cap for `index.max-files`.
    max_files: u64,
    /// Monotonic version counter.
    version: i32,
    /// Rotation counter for corrected operations: always correcting to
    /// the lowest open doc / first free slot collapses every correction
    /// onto one file, hollowing out seed coverage.
    correction_step: usize,
}

impl Default for SessionModel {
    fn default() -> Self {
        let disk_files: BTreeMap<u8, String> = (0..FILES.len() as u8)
            .map(|i| (i, INITIAL_DISK.to_string()))
            .collect();
        Self {
            open_docs: BTreeMap::new(),
            disk_files,
            config_ignore: Vec::new(),
            baseline_suppress: false,
            second_folder: false,
            max_files: 20_000,
            version: 1,
            correction_step: 0,
        }
    }
}

impl SessionModel {
    fn is_open(&self, file: u8) -> bool {
        self.open_docs.contains_key(&file)
    }
    fn has_disk(&self, file: u8) -> bool {
        self.disk_files.contains_key(&file)
    }

    /// Check whether an operation is valid against the current
    /// model state. If false, the operation would be silently skipped
    /// by the executor, reducing coverage without signal.
    fn is_valid(&self, op: &Operation) -> bool {
        match op {
            Operation::Open { file, .. } => !self.is_open(*file) && self.has_disk(*file),
            Operation::FullEdit { file, .. }
            | Operation::IncrementalEdit { file, .. }
            | Operation::RapidEdit { file, .. } => self.is_open(*file),
            Operation::Close { file } => self.is_open(*file),
            Operation::Restart => true,
            Operation::CreateFile { file, .. } => !self.has_disk(*file),
            Operation::DeleteFile { file } => self.has_disk(*file) && !self.is_open(*file),
            Operation::RenameFile { from, to } => {
                *from != *to
                    && self.has_disk(*from)
                    && !self.has_disk(*to)
                    && !self.is_open(*from)
                    && !self.is_open(*to)
            }
            Operation::EditConfig { .. }
            | Operation::EditBaseline { .. }
            | Operation::EditTypeshed
            | Operation::EditNamespace
            | Operation::EditDescription
            | Operation::EditDiscoveryCaps { .. } => true,
            Operation::AddFolder => !self.second_folder,
            Operation::RemoveFolder => self.second_folder,
        }
    }

    /// Generate a valid alternative for an invalid operation.
    /// Each correction advances `correction_step` and picks the
    /// `step % len`-th candidate slot, so consecutive corrections
    /// rotate across open docs / free slots instead of all funneling
    /// onto the first one. Deterministic: the step is model state, and
    /// every seed lane replays the same sequence.
    fn valid_alternative(&mut self, original: &Operation) -> Option<Operation> {
        let step = self.correction_step;
        self.correction_step = self.correction_step.wrapping_add(1);
        let nth = |slots: &[u8]| -> Option<u8> {
            if slots.is_empty() {
                None
            } else {
                Some(slots[step % slots.len()])
            }
        };
        match original {
            Operation::Open { source, .. } => {
                let closed_on_disk: Vec<u8> = (0..FILES.len() as u8)
                    .filter(|&f| !self.is_open(f) && self.has_disk(f))
                    .collect();
                nth(&closed_on_disk).map(|f| Operation::Open {
                    file: f,
                    source: *source,
                })
            }
            Operation::FullEdit { source, .. } => {
                let open: Vec<u8> = self.open_docs.keys().copied().collect();
                nth(&open).map(|f| Operation::FullEdit {
                    file: f,
                    source: *source,
                })
            }
            Operation::IncrementalEdit { source, .. } => {
                let open: Vec<u8> = self.open_docs.keys().copied().collect();
                nth(&open).map(|f| Operation::IncrementalEdit {
                    file: f,
                    source: *source,
                })
            }
            Operation::RapidEdit { source, .. } => {
                let open: Vec<u8> = self.open_docs.keys().copied().collect();
                nth(&open).map(|f| Operation::RapidEdit {
                    file: f,
                    source: *source,
                })
            }
            Operation::Close { .. } => {
                let open: Vec<u8> = self.open_docs.keys().copied().collect();
                nth(&open).map(|f| Operation::Close { file: f })
            }
            Operation::CreateFile { source, .. } => {
                let empty: Vec<u8> = (0..FILES.len() as u8)
                    .filter(|&f| !self.has_disk(f))
                    .collect();
                nth(&empty).map(|f| Operation::CreateFile {
                    file: f,
                    source: *source,
                })
            }
            Operation::DeleteFile { .. } => {
                let deletable: Vec<u8> = (0..FILES.len() as u8)
                    .filter(|&f| self.has_disk(f) && !self.is_open(f))
                    .collect();
                nth(&deletable).map(|f| Operation::DeleteFile { file: f })
            }
            _ => None,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────

/// Build the fixture: primary root with five R files, `ry.toml`,
/// `baseline.json`, and a secondary package root for folder ops.
fn build_fixture() -> FixtureProject {
    let fixture = FixtureProject::empty().unwrap();
    for name in FILES {
        fixture.write_file(*name, INITIAL_DISK).unwrap();
    }
    fixture.write_file("ry.toml", "\n").unwrap();
    fixture
        .write_file("baseline.json", r#"{"version": 1, "entries": []}"#)
        .unwrap();
    // Secondary root — a minimal package so NAMESPACE / DESCRIPTION /
    // typeshed edits exercise real resolution paths.
    fixture
        .write_file(
            format!("{SECOND}/DESCRIPTION"),
            "Package: second\nVersion: 0.0.1\nImports: localdep\n",
        )
        .unwrap();
    fixture
        .write_file(
            format!("{SECOND}/NAMESPACE"),
            "importFrom(localdep, my_func)\nexport(use_it)\n",
        )
        .unwrap();
    fixture
        .write_file(format!("{SECOND}/ry.toml"), "typeshed = [\"typesheds\"]\n")
        .unwrap();
    let stub = serde_json::to_string(&json!({
        "schema_version": "1",
        "package": "localdep",
        "version": "test",
        "functions": {
            "my_func": {
                "params": [],
                "return": {"mode": "integer", "length": "1"}
            }
        }
    }))
    .unwrap();
    fixture
        .write_file(format!("{SECOND}/typesheds/localdep.json"), &stub)
        .unwrap();
    fixture
        .write_file(format!("{SECOND}/R/main.R"), "if (my_func()) print(1)\n")
        .unwrap();
    fixture
}

fn file_path(fixture: &FixtureProject, slot: u8) -> PathBuf {
    fixture.path(FILES[slot as usize])
}

fn file_uri_str(fixture: &FixtureProject, slot: u8) -> String {
    file_uri(&file_path(fixture, slot))
}

/// Write the current model's `ry.toml` to disk.
fn write_config(fixture: &FixtureProject, model: &SessionModel) {
    let mut toml = String::new();
    if !model.config_ignore.is_empty() {
        let items: Vec<String> = model
            .config_ignore
            .iter()
            .map(|r| format!("\"{r}\""))
            .collect();
        toml.push_str(&format!("ignore = [{}]\n", items.join(", ")));
    }
    toml.push_str(&format!("[index]\nmax-files = {}\n", model.max_files));
    std::fs::write(fixture.path("ry.toml"), &toml).unwrap();
}

/// Write the current model's `baseline.json` to disk.
fn write_baseline(fixture: &FixtureProject, model: &SessionModel) {
    let baseline = if model.baseline_suppress {
        r#"{"version": 1, "entries": [{"path": "", "code": "RY090", "message": "", "count": 1}]}"#
    } else {
        r#"{"version": 1, "entries": []}"#
    };
    std::fs::write(fixture.path("baseline.json"), baseline).unwrap();
}

/// Collect the full diagnostic snapshot from a quiesced session: wait for
/// the target URI's publication, then drain all remaining publications.
async fn collect_snapshot(
    session: &mut ClientSession,
    target_uri: &str,
    mark: u64,
) -> BTreeMap<String, Vec<Value>> {
    let raw = session
        .quiesce_diagnostics(target_uri, mark, DRAIN_IDLE)
        .await
        .unwrap_or_default();
    raw.into_iter()
        .map(|(uri, diags)| (uri, sorted_diagnostics(&diags)))
        .collect()
}

/// Spawn a fresh server on the model's current workspace state, open the
/// same documents, and return the diagnostic snapshot.
async fn fresh_server_snapshot(
    fixture: &FixtureProject,
    model: &SessionModel,
    target_slot: u8,
) -> BTreeMap<String, Vec<Value>> {
    let mut roots: Vec<PathBuf> = vec![fixture.root().to_path_buf()];
    if model.second_folder {
        roots.push(fixture.path(SECOND));
    }
    let root_refs: Vec<&Path> = roots.iter().map(|p| p.as_path()).collect();
    let (mut session, server) = spawn_session(&root_refs).await;

    // Sync barrier: drain the initialize/index cycle.
    let target_uri = file_uri_str(fixture, target_slot);
    let _ = session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": &target_uri},
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}
            }),
        )
        .await;

    let mark = session.publication_mark();
    for (&slot, text) in &model.open_docs {
        session
            .open(&file_uri_str(fixture, slot), model.version, text)
            .await
            .unwrap();
    }
    let snapshot = collect_snapshot(&mut session, &target_uri, mark).await;
    join_session(session, server).await;
    snapshot
}

// ──────────────────────────────────────────────────────────────────────────
// Convergence property
// ──────────────────────────────────────────────────────────────────────────

/// Core property: after each quiescent step, the live server's diagnostic
/// snapshot (every URI + discovered-file set) equals a fresh server on the
/// same workspace.
async fn convergence_property(operations: Vec<Operation>) -> Result<(), TestCaseError> {
    let fixture = build_fixture();
    write_config(&fixture, &SessionModel::default());
    write_baseline(&fixture, &SessionModel::default());

    let roots: Vec<&Path> = vec![fixture.root()];
    let (mut live, mut live_server) = spawn_session(&roots).await;
    let mut model = SessionModel::default();

    for (step, operation) in operations.into_iter().enumerate() {
        // Correct an invalid operation by retargeting it within the same
        // operation kind (a different open doc, a different free slot).
        // Ops with no valid same-kind target — RenameFile without a free
        // destination slot, AddFolder when the second folder already
        // exists, RemoveFolder when it does not — have no semantic
        // replacement, so they are deliberately skipped.
        let operation = if !model.is_valid(&operation) {
            match model.valid_alternative(&operation) {
                Some(valid) => valid,
                None => continue,
            }
        } else {
            operation
        };

        // NOTE: target_slot is computed AFTER the operation is applied,
        // because some operations (DeleteFile, RenameFile) change which
        // files exist on disk.

        match &operation {
            // ── Base alphabet ──
            Operation::Open { file, source } => {
                if model.is_open(*file) {
                    continue;
                }
                if !model.has_disk(*file) {
                    continue;
                }
                let text = source.text();
                live.open(&file_uri_str(&fixture, *file), model.version, text)
                    .await
                    .unwrap();
                model.version += 1;
                model.open_docs.insert(*file, text.to_string());
            }
            Operation::FullEdit { file, source } => {
                if !model.is_open(*file) {
                    continue;
                }
                live.change(
                    &file_uri_str(&fixture, *file),
                    model.version,
                    json!([{ "text": source.text() }]),
                )
                .await
                .unwrap();
                model.version += 1;
                model.open_docs.insert(*file, source.text().to_string());
            }
            Operation::IncrementalEdit { file, source } => {
                if !model.is_open(*file) {
                    continue;
                }
                let old_text = &model.open_docs[file];
                let range_end = first_line_utf16_len(old_text);
                live.change(
                    &file_uri_str(&fixture, *file),
                    model.version,
                    json!([{
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": range_end}
                        },
                        "text": source.first_line()
                    }]),
                )
                .await
                .unwrap();
                model.version += 1;
                let new_text = apply_incremental_edit(old_text, *source);
                model.open_docs.insert(*file, new_text);
            }
            Operation::Close { file } => {
                if !model.is_open(*file) {
                    continue;
                }
                let uri = file_uri_str(&fixture, *file);
                model.open_docs.remove(file);
                live.notify(
                    "textDocument/didClose",
                    json!({"textDocument": {"uri": uri}}),
                )
                .await
                .unwrap();
            }
            Operation::Restart => {
                join_session(live, live_server).await;
                let mut roots: Vec<PathBuf> = vec![fixture.root().to_path_buf()];
                if model.second_folder {
                    roots.push(fixture.path(SECOND));
                }
                let root_refs: Vec<&Path> = roots.iter().map(|p| p.as_path()).collect();
                let (new_live, new_server) = spawn_session(&root_refs).await;
                live = new_live;
                live_server = new_server;
                for (&file, text) in &model.open_docs {
                    live.open(&file_uri_str(&fixture, file), model.version, text)
                        .await
                        .unwrap();
                    model.version += 1;
                }
            }

            // ── state-machine extensions ──
            Operation::CreateFile { file, source } => {
                if model.has_disk(*file) {
                    continue;
                }
                fixture
                    .write_file(FILES[*file as usize], source.text())
                    .unwrap();
                model.disk_files.insert(*file, source.text().to_string());
                let ry_toml_uri = file_uri(&fixture.path("ry.toml"));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ry_toml_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::DeleteFile { file } => {
                if !model.has_disk(*file) {
                    continue;
                }
                // Compute URIs while the file still exists (canonicalize
                // needs a real path). Close before deleting so the server
                // removes the document from state.docs.
                let close_uri = if model.is_open(*file) {
                    Some(file_uri_str(&fixture, *file))
                } else {
                    None
                };
                if let Some(uri) = close_uri {
                    model.open_docs.remove(file);
                    live.notify(
                        "textDocument/didClose",
                        json!({"textDocument": {"uri": uri}}),
                    )
                    .await
                    .unwrap();
                }
                let path = file_path(&fixture, *file);
                let _ = std::fs::remove_file(&path);
                model.disk_files.remove(file);
                let ry_toml_uri = file_uri(&fixture.path("ry.toml"));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ry_toml_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::RenameFile { from, to } => {
                // Skip if either file is open: the live server holds open
                // documents under their original URIs, and a rename changes
                // the URI. Closing/reopening under the new URI is a separate
                // operation not in this alphabet.
                if !model.has_disk(*from)
                    || from == to
                    || model.is_open(*from)
                    || model.is_open(*to)
                {
                    continue;
                }
                let from_path = file_path(&fixture, *from);
                let to_path = file_path(&fixture, *to);
                let content = model.disk_files[from].clone();
                let _ = std::fs::remove_file(&to_path);
                std::fs::rename(&from_path, &to_path).unwrap();
                model.disk_files.remove(from);
                model.disk_files.insert(*to, content);
                let ry_toml_uri = file_uri(&fixture.path("ry.toml"));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ry_toml_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::EditConfig { ignore_diagnostic } => {
                model.config_ignore = if *ignore_diagnostic {
                    vec!["RY090".to_string()]
                } else {
                    Vec::new()
                };
                write_config(&fixture, &model);
                let ry_toml_uri = file_uri(&fixture.path("ry.toml"));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ry_toml_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::EditBaseline { suppress } => {
                model.baseline_suppress = *suppress;
                write_baseline(&fixture, &model);
                let baseline_uri = file_uri(&fixture.path("baseline.json"));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": baseline_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::EditNamespace => {
                let content =
                    "importFrom(localdep, my_func)\nimportFrom(localdep, other)\nexport(use_it)\n";
                fixture
                    .write_file(format!("{SECOND}/NAMESPACE"), content)
                    .unwrap();
                let ns_uri = file_uri(&fixture.path(format!("{SECOND}/NAMESPACE")));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ns_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::EditDescription => {
                let content = "Package: second\nVersion: 0.0.2\nImports: localdep\n";
                fixture
                    .write_file(format!("{SECOND}/DESCRIPTION"), content)
                    .unwrap();
                let desc_uri = file_uri(&fixture.path(format!("{SECOND}/DESCRIPTION")));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": desc_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::EditTypeshed => {
                let stub = serde_json::to_string(&json!({
                    "schema_version": "1",
                    "package": "localdep",
                    "version": "test2",
                    "functions": {
                        "my_func": {
                            "params": [],
                            "return": {"mode": "character", "length": "1"}
                        }
                    }
                }))
                .unwrap();
                fixture
                    .write_file(format!("{SECOND}/typesheds/localdep.json"), &stub)
                    .unwrap();
                let ts_uri = file_uri(&fixture.path(format!("{SECOND}/typesheds/localdep.json")));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ts_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::EditDiscoveryCaps { max_files } => {
                model.max_files = *max_files as u64;
                write_config(&fixture, &model);
                let ry_toml_uri = file_uri(&fixture.path("ry.toml"));
                live.notify(
                    "workspace/didChangeWatchedFiles",
                    json!({"changes": [{"uri": ry_toml_uri, "type": 2}]}),
                )
                .await
                .unwrap();
            }
            Operation::RapidEdit { file, source } => {
                if !model.is_open(*file) {
                    continue;
                }
                // Three rapid consecutive edits without quiescing — creates
                // in-flight parses from older generations. The version-
                // stamped tree cache rejects stale results.
                for _ in 0..3 {
                    live.change(
                        &file_uri_str(&fixture, *file),
                        model.version,
                        json!([{ "text": source.text() }]),
                    )
                    .await
                    .unwrap();
                    model.version += 1;
                }
                model.open_docs.insert(*file, source.text().to_string());
            }
            Operation::AddFolder => {
                if model.second_folder {
                    continue;
                }
                let second_uri = file_uri(&fixture.path(SECOND));
                live.notify(
                    "workspace/didChangeWorkspaceFolders",
                    json!({
                        "event": {
                            "added": [{"uri": second_uri, "name": SECOND}],
                            "removed": []
                        }
                    }),
                )
                .await
                .unwrap();
                model.second_folder = true;
            }
            Operation::RemoveFolder => {
                if !model.second_folder {
                    continue;
                }
                let second_uri = file_uri(&fixture.path(SECOND));
                live.notify(
                    "workspace/didChangeWorkspaceFolders",
                    json!({
                        "event": {
                            "added": [],
                            "removed": [{"uri": second_uri, "name": SECOND}]
                        }
                    }),
                )
                .await
                .unwrap();
                model.second_folder = false;
            }
        }

        // ── Quiesce and compare ──
        // Diagnostics are only published through document operations
        // (didOpen / didChange / didClose). When no documents are open,
        // the published set is trivially empty and not observable; skip
        // the comparison. Any latent state change is caught by the next
        // Open, which triggers a full multi-URI broadcast.
        if model.open_docs.is_empty() {
            continue;
        }
        let target_slot = target_for(&operation, &model);
        if let Some(slot) = target_slot {
            let target_uri = file_uri_str(&fixture, slot);
            sync_barrier(&mut live, &target_uri).await;
            let mark = live.publication_mark();
            let live_snap = collect_snapshot(&mut live, &target_uri, mark).await;
            let fresh_snap = fresh_server_snapshot(&fixture, &model, slot).await;
            // Filter empty diagnostics: a URI with [] is equivalent to an
            // absent URI. This handles the didClose empty-publish for
            // deleted files and clean files that the fresh server
            // publishes with [].  Only non-empty diagnostic content
            // matters for convergence.
            let live_filtered: BTreeMap<&str, &Vec<Value>> = live_snap
                .iter()
                .filter(|(_, diags)| !diags.is_empty())
                .map(|(k, v)| (k.as_str(), v))
                .collect();
            let fresh_filtered: BTreeMap<&str, &Vec<Value>> = fresh_snap
                .iter()
                .filter(|(_, diags)| !diags.is_empty())
                .map(|(k, v)| (k.as_str(), v))
                .collect();
            prop_assert_eq!(
                live_filtered,
                fresh_filtered,
                "snapshot mismatch after step {} ({:?}) for {}",
                step,
                operation,
                target_uri
            );
        }
    }

    join_session(live, live_server).await;
    Ok(())
}

/// Choose the target slot for comparison: a file that exists on disk or is
/// open, relevant to the operation. For delete/rename, the target is a
/// *surviving* file, not the removed one.
fn target_for(operation: &Operation, model: &SessionModel) -> Option<u8> {
    let primary = match operation {
        Operation::Open { file, .. }
        | Operation::FullEdit { file, .. }
        | Operation::IncrementalEdit { file, .. }
        | Operation::CreateFile { file, .. }
        | Operation::RapidEdit { file, .. } => Some(*file),
        Operation::RenameFile { to, .. } => Some(*to),
        // Close and DeleteFile target a *surviving* file: the closed/
        // deleted file gets an immediate empty publish (before the mark),
        // so waiting for it would time out. Target the first remaining
        // open doc so the quiesce captures the debounced re-publication.
        Operation::Close { .. } | Operation::DeleteFile { .. } => None,
        _ => None,
    };
    if let Some(slot) = primary
        && (model.has_disk(slot) || model.is_open(slot))
    {
        return Some(slot);
    }
    // Fallback: first open doc, or first disk file.
    if let Some((&slot, _)) = model.open_docs.iter().next() {
        return Some(slot);
    }
    if let Some((&slot, _)) = model.disk_files.iter().next() {
        return Some(slot);
    }
    None
}

// ──────────────────────────────────────────────────────────────────────────
// Deterministic seed infrastructure
// ──────────────────────────────────────────────────────────────────────────

/// Convert a u64 seed to the 32-byte array ChaCha requires.
fn seed_bytes(seed: u64) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    bytes
}

/// Run the convergence property on a single deterministic seed. Uses a
/// `TestRunner` with a fixed ChaCha RNG so generation is reproducible and
/// shrinking is automatic.
fn run_deterministic_seed(seed: u64) {
    let config = Config {
        cases: 1,
        max_shrink_iters: 512,
        failure_persistence: None,
        source_file: None,
        ..Config::default()
    };
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &seed_bytes(seed));
    let mut runner = TestRunner::new_with_rng(config, rng);
    let result = runner.run(&operation_sequence_strategy(), |operations| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(convergence_property(operations))
    });
    match result {
        Ok(()) => {}
        Err(e) => panic!("seed {seed:#018x} failed:\n{e}"),
    }
}

/// Bounded PR seed set. These are explicit, committed seeds — no random
/// generation. Enough seeds to exercise every operation type in the first
/// few positions with high probability, while keeping PR CI fast.
const PR_SEEDS: &[u64] = &[
    0x0000_0000_0000_0001,
    0x0000_0000_0000_0002,
    0x0000_0000_0000_0003,
    0x0000_0000_0000_0004,
    0x0000_0000_0000_0005,
    0x0000_0000_0000_0006,
    0x1111_1111_1111_1111,
    0x2222_2222_2222_2222,
    0x3333_3333_3333_3333,
    0x4444_4444_4444_4444,
    0x5555_5555_5555_5555,
    0x6666_6666_6666_6666,
    0x7777_7777_7777_7777,
    0x8888_8888_8888_8888,
    0x1234_5678_9abc_def0,
    0x0fed_cba9_8765_4321,
];

#[test]
fn pr_session_seeds() {
    for &seed in PR_SEEDS {
        run_deterministic_seed(seed);
    }
}

/// 1,000 fixed nightly seeds. `#[ignore]`'d because it takes minutes.
/// Run with: `cargo test -p ry-lsp --test session_state_machine -- --ignored nightly`
#[test]
#[ignore = "Nightly: 1,000 fixed seeds — run explicitly"]
fn nightly_session_seeds() {
    for i in 1..=1000u64 {
        run_deterministic_seed(i);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Historical bug regression: every pre-release bug caught by a deterministic case
// ──────────────────────────────────────────────────────────────────────────

/// Helper: build an explicit operation sequence and run it. Used for
/// targeted regression cases that exercise specific historical bug paths.
fn run_explicit_sequence(operations: Vec<Operation>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(convergence_property(operations))
        .unwrap_or_else(|e| panic!("explicit sequence failed: {e:?}"));
}

/// #53 (version-stamped tree cache): rapid edits create in-flight parses
/// from older generations. A regression that serves stale trees produces
/// wrong diagnostics that diverge from a fresh server.
#[test]
fn catches_issue_53_stale_parse() {
    run_explicit_sequence(vec![
        Operation::Open {
            file: 0,
            source: SourceVariant::Clean,
        },
        Operation::RapidEdit {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
        Operation::IncrementalEdit {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
    ]);
}

/// #55 (workspace-folder mutation): removing a folder must clear its
/// diagnostics. A regression leaves stale diagnostics from the removed root.
#[test]
fn catches_issue_55_folder_mutation() {
    run_explicit_sequence(vec![
        Operation::AddFolder,
        Operation::RemoveFolder,
        Operation::Open {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
    ]);
}

/// #45 (baseline reload): editing the baseline must converge to the new
/// value. A regression that retains stale baseline state diverges.
#[test]
fn catches_issue_45_baseline_reload() {
    run_explicit_sequence(vec![
        Operation::Open {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
        Operation::EditBaseline { suppress: true },
        Operation::FullEdit {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
    ]);
}

/// #48 (bounded discovery): a tight discovery cap must limit the discovered
/// file set. A regression that ignores the cap discovers more files than a
/// fresh server with the same cap.
#[test]
fn catches_issue_48_discovery_caps() {
    run_explicit_sequence(vec![
        Operation::EditDiscoveryCaps { max_files: 2 },
        Operation::Open {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
    ]);
}

/// #44 / #56 (per-folder config): editing config changes diagnostic
/// filtering. A regression that doesn't reload config diverges from a
/// fresh server.
#[test]
fn catches_config_reload() {
    run_explicit_sequence(vec![
        Operation::Open {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
        Operation::EditConfig {
            ignore_diagnostic: true,
        },
        Operation::FullEdit {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
    ]);
}

/// On-disk file lifecycle: create, delete, and rename must converge.
#[test]
fn catches_disk_file_lifecycle() {
    run_explicit_sequence(vec![
        Operation::DeleteFile { file: 4 },
        Operation::Open {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
        Operation::CreateFile {
            file: 4,
            source: SourceVariant::AsciiDiagnostic,
        },
        Operation::RenameFile { from: 4, to: 3 },
    ]);
}

/// Server restart must converge to the same state.
#[test]
fn catches_restart_convergence() {
    run_explicit_sequence(vec![
        Operation::Open {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
        Operation::Restart,
        Operation::IncrementalEdit {
            file: 0,
            source: SourceVariant::AstralDiagnostic,
        },
    ]);
}

/// Metadata operations (NAMESPACE/DESCRIPTION/typeshed) exercise the
/// didChangeWatchedFiles resolution-reload path. They are not in the random
/// alphabet because they require a package-structured fixture, but they are
/// part of the state-machine alphabet and must converge.
#[test]
fn metadata_edits_converge() {
    run_explicit_sequence(vec![
        Operation::AddFolder,
        Operation::Open {
            file: 0,
            source: SourceVariant::Clean,
        },
        Operation::EditNamespace,
        Operation::EditDescription,
        Operation::EditTypeshed,
        Operation::IncrementalEdit {
            file: 0,
            source: SourceVariant::AsciiDiagnostic,
        },
    ]);
}

// ──────────────────────────────────────────────────────────────────────────
// Deterministic session tests
//
// Targeted coverage the property only samples randomly: exact UTF-16
// positions across a non-ASCII transcript, and the #81 close/reopen
// republication race. These tests predate the property: the original
// open/edit/close/restart convergence property they accompanied was
// deleted as subsumed by `pr_session_seeds`.
// ──────────────────────────────────────────────────────────────────────────

/// The transcript document: CRLF terminators, BMP text, a decomposed
/// combining mark, astral-plane characters in strings and backtick
/// identifiers, plus two diagnostic triggers.
const SOURCE: &str = concat!(
    "ascii <- 1L\r\n",
    "frame <- data.frame(column = 1L)\r\n",
    "marker <- \"é e\u{301} 😀\"; target <- 1L\r\n",
    "marker; target\r\n",
    "marker <- \"😀\"; length(xx = 1L)\r\n",
    "`😀` <- 4L\r\n",
    "`😀`\r\n",
);
const OTHER_SOURCE: &str = "\"😀\"; target\r\n";
const DISK_SOURCE: &str = "marker <- \"😀\"; length(xx = 1L)\r\n";

/// A hand-declared point in `SOURCE`. These values intentionally do not use
/// ry-lsp or ry-testkit conversion helpers: byte offsets count UTF-8 bytes
/// and character columns count UTF-16 code units.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    name: &'static str,
    byte: usize,
    line: u32,
    scalar: u32,
    character: u32,
    following: &'static str,
}

// Hand-declared from the literal above.
const ANCHORS: &[Anchor] = &[
    Anchor {
        name: "ascii",
        byte: 0,
        line: 0,
        scalar: 0,
        character: 0,
        following: "ascii",
    },
    Anchor {
        name: "line after CRLF",
        byte: 13,
        line: 1,
        scalar: 0,
        character: 0,
        following: "frame",
    },
    Anchor {
        name: "BMP start",
        byte: 58,
        line: 2,
        scalar: 11,
        character: 11,
        following: "é",
    },
    Anchor {
        name: "BMP end",
        byte: 60,
        line: 2,
        scalar: 12,
        character: 12,
        following: " ",
    },
    Anchor {
        name: "combining base",
        byte: 61,
        line: 2,
        scalar: 13,
        character: 13,
        following: "e",
    },
    Anchor {
        name: "combining mark",
        byte: 62,
        line: 2,
        scalar: 14,
        character: 14,
        following: "\u{301}",
    },
    Anchor {
        name: "combining end",
        byte: 64,
        line: 2,
        scalar: 15,
        character: 15,
        following: " ",
    },
    Anchor {
        name: "astral start",
        byte: 65,
        line: 2,
        scalar: 16,
        character: 16,
        following: "😀",
    },
    Anchor {
        name: "astral end",
        byte: 69,
        line: 2,
        scalar: 17,
        character: 18,
        following: "\"",
    },
    Anchor {
        name: "target declaration",
        byte: 72,
        line: 2,
        scalar: 20,
        character: 21,
        following: "target",
    },
    Anchor {
        name: "target declaration end",
        byte: 78,
        line: 2,
        scalar: 26,
        character: 27,
        following: " ",
    },
    Anchor {
        name: "target read",
        byte: 94,
        line: 3,
        scalar: 8,
        character: 8,
        following: "target",
    },
    Anchor {
        name: "target read end",
        byte: 100,
        line: 3,
        scalar: 14,
        character: 14,
        following: "\r\n",
    },
    Anchor {
        name: "diagnostic name",
        byte: 127,
        line: 4,
        scalar: 22,
        character: 23,
        following: "xx",
    },
    Anchor {
        name: "diagnostic name end",
        byte: 129,
        line: 4,
        scalar: 24,
        character: 25,
        following: " =",
    },
    Anchor {
        name: "diagnostic end",
        byte: 134,
        line: 4,
        scalar: 29,
        character: 30,
        following: ")",
    },
    Anchor {
        name: "backtick astral",
        byte: 138,
        line: 5,
        scalar: 1,
        character: 1,
        following: "😀",
    },
    Anchor {
        name: "backtick astral end",
        byte: 142,
        line: 5,
        scalar: 2,
        character: 3,
        following: "`",
    },
    Anchor {
        name: "multiline backtick read",
        byte: 152,
        line: 6,
        scalar: 1,
        character: 1,
        following: "😀",
    },
];

fn anchor(name: &str) -> Anchor {
    *ANCHORS.iter().find(|anchor| anchor.name == name).unwrap()
}

fn position(name: &str) -> Value {
    let anchor = anchor(name);
    json!({"line": anchor.line, "character": anchor.character})
}

fn assert_position(actual: &Value, name: &str) {
    assert_eq!(actual, &position(name), "wrong LSP position for {name}");
}

/// Request inlay hints for a single line of `uri`. The transcript test
/// uses this as its state-observable probe: a response is produced from
/// the cached parse and the checked scope, so it reflects exactly what
/// the server currently believes the document contains.
async fn hints_for_line(session: &mut ClientSession, uri: &str, line: u32) -> Value {
    session
        .request(
            "textDocument/inlayHint",
            json!({
                "textDocument": {"uri": uri},
                "range": {
                    "start": {"line": line, "character": 0},
                    "end": {"line": line + 1, "character": 0}
                }
            }),
        )
        .await
        .unwrap()
}

/// Assert the server placed a type hint immediately after the binding
/// identifier at (`line`, `character`).
fn assert_hint_at(hints: &Value, line: u32, character: u32) {
    assert!(
        hints
            .as_array()
            .expect("inlay hints should be an array")
            .iter()
            .any(
                |hint| hint["position"] == json!({"line": line, "character": character})
                    && hint["label"]
                        .as_str()
                        .is_some_and(|label| label.starts_with(": "))
            ),
        "expected a type hint at ({line}, {character}); got: {hints}"
    );
}

#[test]
fn utf16_contract_holds_across_one_real_lsp_transcript() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(utf16_transcript());
}

async fn utf16_transcript() {
    for anchor in ANCHORS {
        assert!(
            SOURCE.as_bytes()[anchor.byte..].starts_with(anchor.following.as_bytes()),
            "bad independent byte offset for {}",
            anchor.name
        );
        let prefix = &SOURCE[..anchor.byte];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
        let line_start = prefix.rfind('\n').map_or(0, |byte| byte + 1);
        let line_prefix = &SOURCE[line_start..anchor.byte];
        assert_eq!(line, anchor.line, "bad line for {}", anchor.name);
        assert_eq!(
            line_prefix.chars().count() as u32,
            anchor.scalar,
            "bad Unicode-scalar column for {}",
            anchor.name
        );
        assert_eq!(
            line_prefix.encode_utf16().count() as u32,
            anchor.character,
            "bad UTF-16 column for {}",
            anchor.name
        );
    }

    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", SOURCE).unwrap();
    fixture.write_file("other.R", OTHER_SOURCE).unwrap();
    fixture.write_file("disk.R", DISK_SOURCE).unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    let other_uri = file_uri(&fixture.path("other.R"));
    let disk_uri = file_uri(&fixture.path("disk.R"));
    let (client_stream, server_stream) = tokio::io::duplex(128 * 1024);
    let (client_reader, client_writer) = tokio::io::split(client_stream);
    let (server_reader, server_writer) = tokio::io::split(server_stream);
    let server = tokio::spawn(async move { ry_lsp::run_with(server_reader, server_writer).await });
    let mut session = LspSession::new(client_reader, client_writer);
    let initialize = session.initialize(fixture.root()).await.unwrap();
    assert_eq!(
        initialize.pointer("/capabilities/positionEncoding"),
        Some(&json!("utf-16"))
    );
    let open_mark = session.publication_mark();
    session.open(&main_uri, 1, SOURCE).await.unwrap();
    session.open(&other_uri, 1, OTHER_SOURCE).await.unwrap();

    // Byte -> UTF-16: diagnostics publish the exact range after BMP,
    // combining, astral, and CRLF prefixes.
    let publish = session
        .published_diagnostics_after(&main_uri, open_mark)
        .await
        .unwrap();
    let diagnostic = publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "RY090")
        .expect("length's partial argument name should emit RY090");
    assert_position(&diagnostic["range"]["start"], "diagnostic name");
    assert_position(&diagnostic["range"]["end"], "diagnostic end");

    // Unopened indexed files must retain their source text too; otherwise
    // byte columns leak into LSP ranges.
    let disk_publish = session
        .published_diagnostics_after(&disk_uri, open_mark)
        .await
        .unwrap();
    let disk_diagnostic = disk_publish["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "RY090")
        .expect("indexed file should publish RY090");
    assert_eq!(
        disk_diagnostic["range"],
        json!({
            "start": {"line": 0, "character": 23},
            "end": {"line": 0, "character": 30}
        })
    );

    // The state-observable probe through a kept capability: the line-0
    // binding places a type hint right after its identifier. The
    // change-path tests below reuse this probe to observe what the
    // server's cached parse contains after each edit.
    let initial_hints = hints_for_line(&mut session, &main_uri, 0).await;
    assert_hint_at(&initial_hints, 0, 5);

    // The document-change path must reject the same invalid position rather
    // than corrupting the document by snapping into the backtick identifier.
    let invalid_change_mark = session.publication_mark();
    session
        .change(
            &main_uri,
            2,
            json!([{
                "range": {
                    "start": {"line": 6, "character": 2},
                    "end": {"line": 6, "character": 3}
                },
                "text": "BROKEN"
            }]),
        )
        .await
        .unwrap();
    session
        .published_diagnostics_after(&main_uri, invalid_change_mark)
        .await
        .unwrap();
    // The rejected edit must leave the last-good parse serving content
    // requests unchanged.
    let hints_after_invalid_change = hints_for_line(&mut session, &main_uri, 0).await;
    assert_hint_at(&hints_after_invalid_change, 0, 5);

    let out_of_range_change_mark = session.publication_mark();
    session
        .change(
            &main_uri,
            3,
            json!([{
                "range": {
                    "start": {"line": 6, "character": 99},
                    "end": {"line": 6, "character": 99}
                },
                "text": "BROKEN"
            }]),
        )
        .await
        .unwrap();
    session
        .published_diagnostics_after(&main_uri, out_of_range_change_mark)
        .await
        .unwrap();
    let hints_after_out_of_range_change = hints_for_line(&mut session, &main_uri, 0).await;
    assert_hint_at(&hints_after_out_of_range_change, 0, 5);

    // A valid incremental edit after BMP, decomposed combining, and astral
    // scalars must use UTF-16 columns in both endpoints.
    let valid_change_mark = session.publication_mark();
    session
        .change(
            &main_uri,
            4,
            json!([{
                "range": {
                    "start": {"line": 2, "character": 21},
                    "end": {"line": 2, "character": 27}
                },
                "text": "changed"
            }]),
        )
        .await
        .unwrap();
    let changed_publish = session
        .published_diagnostics_after(&main_uri, valid_change_mark)
        .await
        .unwrap();
    assert!(
        changed_publish["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "RY090")
    );

    // The re-parse is visible through the kept surface: the edited
    // line-2 binding is now `changed`, so its hint moves to the new
    // identifier's UTF-16 end column.
    let hints_after_valid_change = hints_for_line(&mut session, &main_uri, 2).await;
    assert_hint_at(&hints_after_valid_change, 2, 28);

    session.shutdown().await.unwrap();
    drop(session);
    tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Regression for #81: after a close/reopen the server must publish the
/// reopened document's diagnostics, and the close's clearing `[]` must not
/// be mistakable for that publication.  Deterministic companion to the
/// convergence property, which samples this path randomly.
#[test]
fn close_reopen_publishes_diagnostics_again() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(close_reopen_republishes());
}

async fn close_reopen_republishes() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", INITIAL_DISK).unwrap();
    let uri = file_uri(&fixture.path("a.R"));
    let (mut session, server) = spawn_session(&[fixture.root()]).await;

    // Barrier + mark before the open, the same pattern
    // `fresh_server_snapshot` uses to drain the initialize cycle's
    // background-index publications.
    sync_barrier(&mut session, &uri).await;
    let open_mark = session.publication_mark();
    session
        .open(&uri, 1, SourceVariant::AsciiDiagnostic.text())
        .await
        .unwrap();
    let first = session
        .published_diagnostics_after(&uri, open_mark)
        .await
        .unwrap();
    assert!(
        !normalize_diagnostics(&first).is_empty(),
        "initial open must publish diagnostics"
    );

    let close_mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
        .await
        .unwrap();
    let cleared = session
        .published_diagnostics_after(&uri, close_mark)
        .await
        .unwrap();
    assert!(
        normalize_diagnostics(&cleared).is_empty(),
        "close must clear diagnostics"
    );

    let reopen_mark = session.publication_mark();
    session
        .open(&uri, 2, SourceVariant::AstralDiagnostic.text())
        .await
        .unwrap();
    let republish = session
        .published_diagnostics_after(&uri, reopen_mark)
        .await
        .unwrap();
    assert!(
        !normalize_diagnostics(&republish).is_empty(),
        "reopen must publish diagnostics again, not the close's clearing []"
    );

    join_session(session, server).await;
}
