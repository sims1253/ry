//! Opt-in, bounded execution telemetry for `Project`.
//!
//! Events describe scheduling decisions as they happen. They are not proof
//! trees and their function IDs identify winning table bindings only within
//! the stated source snapshot; they are not rename/navigation symbol IDs.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use serde::Serialize;
use sha2::{Digest, Sha256};

const EVENT_MARKER_RESERVE: usize = 128;

fn hex_digest(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut hex, "{byte:02x}").expect("write to String");
    }
    hex
}

/// Limits and exact filters for a single project trace. Tracing is off until
/// [`crate::Project::enable_trace`] is called.
#[derive(Clone, Debug)]
pub struct TraceOptions {
    /// Maximum number of logical events, including a truncation marker.
    pub max_events: usize,
    /// Maximum JSON bytes for the event array, including brackets and commas.
    pub max_event_bytes: usize,
    /// Keep file-specific events only for this logical project path.
    pub file: Option<String>,
    /// Keep function-specific events only for this exact snapshot-local ID.
    /// Global round/completion events remain visible.
    pub function: Option<TraceFunctionId>,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self {
            max_events: 512,
            max_event_bytes: 64 * 1024,
            file: None,
            function: None,
        }
    }
}

impl TraceOptions {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !(1..=10_000).contains(&self.max_events) {
            return Err("trace max_events must be 1..=10000");
        }
        if !(256..=1024 * 1024).contains(&self.max_event_bytes) {
            return Err("trace max_event_bytes must be 256..=1048576");
        }
        Ok(())
    }
}

/// Logical source path, project order, and a digest of source text. The path
/// is the same one given to `Project`, not an inferred physical file identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct TraceFileId {
    pub index: usize,
    pub path: String,
    pub source_sha256: String,
}

impl TraceFileId {
    pub(crate) fn from_source(index: usize, path: &str, source: &str) -> Self {
        Self {
            index,
            path: path.to_owned(),
            source_sha256: hex_digest(&Sha256::digest(source.as_bytes())),
        }
    }
}

/// Winning binding in the project's merged function table. Later files with
/// the same name replace earlier ones; the ID describes the winner. It is
/// stable within this source snapshot, not across edits or renames.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct TraceFunctionId {
    pub file: TraceFileId,
    pub table_name: String,
}

/// The input that caused a scheduling or emission decision. A return slot
/// can have several table names; the slot is the actual read identity and
/// `bindings` lists its winning snapshot-local names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceTrigger {
    Function {
        identity: TraceFunctionId,
    },
    ReturnSlot {
        slot: usize,
        bindings: Vec<TraceFunctionId>,
    },
    UnresolvedFunction {
        table_name: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceCompletion {
    Converged,
    BoundReached,
    /// Reserved for an entry point with supported cancellation. The current
    /// synchronous `Project` methods cannot cancel and never emit this.
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceReason {
    ColdStart,
    DirtySource,
    RemovedDefinition,
    ReplacedDefinition,
    PriorAttachmentDiscovery,
    PackageSetChanged,
    CallableContextChanged,
    NoDirtyWork,
    DependencyRead,
    ReturnChanged,
    ReturnAndEvaluationMetadataChanged,
    EvaluationMetadataChanged,
    PackageAttachmentChanged,
    FullScopeRetry,
    GlobalContextChanged,
    S3MethodChanged,
    ReferenceCapture,
}

/// Event payloads are all logical facts; durations live outside this stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEventKind {
    Collection {
        cache_hit: bool,
    },
    Invalidation {
        reason: TraceReason,
    },
    RefinementScope {
        full: bool,
        size: usize,
        reason: TraceReason,
    },
    Round {
        round: usize,
        pending_before: usize,
        /// Function-body refinement attempts in this round, including repeats.
        refined: usize,
        return_changes: usize,
        metadata_changes: usize,
        attachments_changed: bool,
        pending_after: usize,
    },
    Scheduled {
        round: usize,
        reason: TraceReason,
    },
    FixpointComplete {
        completion: TraceCompletion,
        rounds: usize,
    },
    FunctionChanged {
        return_changed: bool,
        signature_changed: bool,
    },
    Emission {
        reason: TraceReason,
    },
    Truncated {
        dropped_events: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TraceEvent {
    #[serde(flatten)]
    pub kind: TraceEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<TraceFileId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<TraceFunctionId>,
    /// Set when the target's definition cannot be tied to a retained source
    /// snapshot. This is never treated as a global event by filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_function: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TraceTrigger>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TraceSummary {
    pub collected_files: usize,
    pub collection_cache_hits: usize,
    pub refinement_rounds: usize,
    /// Function-body refinement attempts across rounds, not unique functions
    /// or changed results.
    pub refined_functions: usize,
    pub emitted_files: usize,
    pub emission_cache_hits: usize,
    pub completion: Option<TraceCompletion>,
    pub truncated: bool,
    pub dropped_events: usize,
    pub event_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectTrace {
    pub schema_version: u32,
    /// Monotonic only within one `Project` instance. Set this to a common
    /// value when comparing equivalent independent runs.
    pub analysis_run_id: u64,
    /// SHA-256 of ordered logical paths, their source digests, and declared
    /// package names. External stub content is not included.
    pub input_snapshot_sha256: String,
    pub summary: TraceSummary,
    pub events: Vec<TraceEvent>,
}

pub(crate) struct TraceRecorder {
    options: TraceOptions,
    run_id: u64,
    input_hash: Sha256,
    files: HashMap<String, TraceFileId>,
    functions: HashMap<String, TraceFunctionId>,
    previous_functions: HashMap<String, TraceFunctionId>,
    events: Vec<TraceEvent>,
    event_bytes: usize,
    dropped_events: usize,
    pub(crate) summary: TraceSummary,
}

pub(crate) struct RoundMetrics {
    pub(crate) round: usize,
    pub(crate) pending_before: usize,
    pub(crate) refined: usize,
    pub(crate) return_changes: usize,
    pub(crate) metadata_changes: usize,
    pub(crate) attachments_changed: bool,
    pub(crate) pending_after: usize,
}

impl TraceRecorder {
    pub(crate) fn new(
        options: TraceOptions,
        run_id: u64,
        previous_functions: HashMap<String, TraceFunctionId>,
        declared_packages: &HashSet<String>,
    ) -> Self {
        let mut input_hash = Sha256::new();
        let mut packages: Vec<_> = declared_packages.iter().collect();
        packages.sort_unstable();
        for package in packages {
            input_hash.update((package.len() as u64).to_le_bytes());
            input_hash.update(package.as_bytes());
        }
        Self {
            options,
            run_id,
            input_hash,
            files: HashMap::new(),
            functions: HashMap::new(),
            previous_functions,
            events: Vec::new(),
            event_bytes: 2, // JSON array brackets
            dropped_events: 0,
            summary: TraceSummary::default(),
        }
    }

    pub(crate) fn register_file<'a>(
        &mut self,
        index: usize,
        path: &str,
        source: &str,
        names: impl Iterator<Item = &'a str>,
    ) {
        let id = TraceFileId::from_source(index, path, source);
        self.input_hash.update((path.len() as u64).to_le_bytes());
        self.input_hash.update(path.as_bytes());
        self.input_hash.update(id.source_sha256.as_bytes());
        let mut names: Vec<_> = names.collect();
        names.sort_unstable();
        for name in names {
            self.functions.insert(
                name.to_owned(),
                TraceFunctionId {
                    file: id.clone(),
                    table_name: name.to_owned(),
                },
            );
        }
        self.files.insert(path.to_owned(), id);
    }

    pub(crate) fn function_id(&self, name: &str) -> Option<TraceFunctionId> {
        self.functions
            .get(name)
            .or_else(|| self.previous_functions.get(name))
            .cloned()
    }

    pub(crate) fn collection(&mut self, path: &str, cache_hit: bool) {
        if cache_hit {
            self.summary.collection_cache_hits += 1;
        } else {
            self.summary.collected_files += 1;
        }
        self.file_event(path, TraceEventKind::Collection { cache_hit });
    }

    pub(crate) fn file_event(&mut self, path: &str, kind: TraceEventKind) {
        self.file_event_with_trigger(path, kind, None);
    }

    pub(crate) fn file_event_with_trigger(
        &mut self,
        path: &str,
        kind: TraceEventKind,
        trigger: Option<TraceTrigger>,
    ) {
        let file = self.files.get(path).cloned();
        self.push(TraceEvent {
            kind,
            unresolved_file: file.is_none().then(|| path.to_owned()),
            file,
            function: None,
            unresolved_function: None,
            trigger,
        });
    }

    pub(crate) fn function_event(&mut self, name: &str, kind: TraceEventKind) {
        self.function_event_with_trigger(name, kind, None);
    }

    pub(crate) fn function_event_with_trigger(
        &mut self,
        name: &str,
        kind: TraceEventKind,
        trigger: Option<TraceTrigger>,
    ) {
        let function = self.function_id(name);
        self.push(TraceEvent {
            kind,
            file: None,
            unresolved_file: None,
            unresolved_function: function.is_none().then(|| name.to_owned()),
            function,
            trigger,
        });
    }

    pub(crate) fn function_trigger(&self, name: &str) -> TraceTrigger {
        self.function_id(name).map_or_else(
            || TraceTrigger::UnresolvedFunction {
                table_name: name.to_owned(),
            },
            |identity| TraceTrigger::Function { identity },
        )
    }

    pub(crate) fn return_slot_trigger<'a>(
        &self,
        slot: usize,
        names: impl Iterator<Item = &'a str>,
    ) -> TraceTrigger {
        TraceTrigger::ReturnSlot {
            slot,
            bindings: names.filter_map(|name| self.function_id(name)).collect(),
        }
    }

    pub(crate) fn global_event(&mut self, kind: TraceEventKind) {
        self.push(TraceEvent {
            kind,
            file: None,
            unresolved_file: None,
            function: None,
            unresolved_function: None,
            trigger: None,
        });
    }

    pub(crate) fn round(&mut self, metrics: RoundMetrics) {
        self.summary.refinement_rounds += 1;
        self.summary.refined_functions += metrics.refined;
        self.global_event(TraceEventKind::Round {
            round: metrics.round,
            pending_before: metrics.pending_before,
            refined: metrics.refined,
            return_changes: metrics.return_changes,
            metadata_changes: metrics.metadata_changes,
            attachments_changed: metrics.attachments_changed,
            pending_after: metrics.pending_after,
        });
    }

    pub(crate) fn completion(&mut self, completion: TraceCompletion, rounds: usize) {
        self.summary.completion = Some(completion);
        self.global_event(TraceEventKind::FixpointComplete { completion, rounds });
    }

    fn push(&mut self, event: TraceEvent) {
        if let Some(filter) = &self.options.file {
            let file = event
                .file
                .as_ref()
                .or_else(|| event.function.as_ref().map(|function| &function.file));
            if event.unresolved_file.is_some()
                || event.unresolved_function.is_some()
                || file.is_some_and(|file| &file.path != filter)
            {
                return;
            }
        }
        if let Some(filter) = &self.options.function {
            if event
                .function
                .as_ref()
                .is_some_and(|function| function != filter)
                || (event.function.is_none()
                    && (event.file.is_some()
                        || event.unresolved_file.is_some()
                        || event.unresolved_function.is_some()))
            {
                return;
            }
        }
        // Keep a prefix. Once either budget is exhausted, count later
        // eligible events without serializing or retaining their payloads.
        if self.dropped_events > 0 || self.events.len() >= self.options.max_events - 1 {
            self.dropped_events += 1;
            return;
        }
        let serialized = serde_json::to_vec(&event).expect("trace event is serializable");
        let additional = serialized.len() + usize::from(!self.events.is_empty());
        if self.event_bytes.saturating_add(additional)
            <= self.options.max_event_bytes - EVENT_MARKER_RESERVE
        {
            self.event_bytes += additional;
            self.events.push(event);
        } else {
            self.dropped_events += 1;
        }
    }

    pub(crate) fn finish(mut self) -> (ProjectTrace, HashMap<String, TraceFunctionId>) {
        if self.dropped_events > 0 {
            let marker = TraceEvent {
                kind: TraceEventKind::Truncated {
                    dropped_events: self.dropped_events,
                },
                file: None,
                unresolved_file: None,
                function: None,
                unresolved_function: None,
                trigger: None,
            };
            let additional = serde_json::to_vec(&marker)
                .expect("truncation marker is serializable")
                .len()
                + usize::from(!self.events.is_empty());
            assert!(additional <= EVENT_MARKER_RESERVE);
            self.event_bytes += additional;
            self.events.push(marker);
        }
        self.summary.truncated = self.dropped_events > 0;
        self.summary.dropped_events = self.dropped_events;
        self.summary.event_bytes = self.event_bytes;
        let report = ProjectTrace {
            schema_version: 1,
            analysis_run_id: self.run_id,
            input_snapshot_sha256: hex_digest(&self.input_hash.finalize()),
            summary: self.summary,
            events: self.events,
        };
        (report, self.functions)
    }
}
