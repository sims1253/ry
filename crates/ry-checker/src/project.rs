//! Project-level checking: shares the `FnTable` and S3 methods table
//! across multiple files in a project.
//!
//! `Checker` is single-file: it builds a fresh `FnTable` for each file
//! it checks, so a function defined in `utils.R` is not visible when
//! checking `analysis.R`. `Project` fixes that by:
//!
//! 1. Collecting function definitions from every file into a single
//!    shared `FnTable` (pass 1).
//! 2. Running the fixpoint loop over the shared table so cross-file
//!    return-type inference converges (pass 2).
//! 3. Walking each file's top-level statements against the refined
//!    shared table to emit per-file diagnostics (pass 3).
//!
//! Backward compatibility: `Checker` continues to work unchanged for
//! single-file use cases (the corpus harness and the existing unit
//! tests rely on this).

use crate::{CallerVisibleSignature, Checker, Diagnostic, FnTable, ReturnSlots};
use rayon::prelude::*;
use ry_core::SourceFile;
use ry_typeshed::Typeshed;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

struct FileEmission {
    index: usize,
    path: String,
    diagnostics: Vec<Diagnostic>,
    scopes: Vec<crate::ScopeRecord>,
    references: crate::ReferenceFacts,
    read_fns: HashSet<String>,
}

/// A multi-file R project. Functions defined in any file are visible
/// to all other files. The fixpoint loop refines returns across the
/// whole project at once.
///
/// Files are checked in the order they were added with [`Project::add_file`].
/// That ordering matters for shadowing semantics: if two files define
/// a top-level function with the same name, the later `add_file` wins
/// (matching R's own `source()` ordering, where the most recently
/// sourced file's bindings override earlier ones).
#[derive(Default)]
pub struct Project {
    /// Shared function table. Populated by pass 1 from all files, then
    /// refined by pass 2. Kept on `Project` rather than recreated each
    /// iteration so callers can re-check after edits if needed.
    fn_table: FnTable,
    /// Shared inferred return types, refined by pass 2's fixpoint loop.
    return_slots: ReturnSlots,
    /// Per-file source, keyed by path. We keep these around so pass 3
    /// (diagnostic emission) has each file's AST in hand.
    files: Vec<(String, Arc<SourceFile>)>,
    /// Cached per-file diagnostics from the most recent `check()` call.
    /// Serves `check_incremental`, which reuses them for files outside
    /// the dirty set instead of re-checking those files.
    diagnostics: Vec<(String, Vec<Diagnostic>)>,
    /// Packages declared in `ry.toml`'s `packages` key, unioned at
    /// `check()` time with packages attached via `library`/`require` in
    /// any file. Seeded into every pass-3 emitter
    /// so the dplyr NSE gating sees a project-wide view.
    loaded: std::collections::HashSet<String>,
    /// Packages explicitly configured by the caller. Kept separate from
    /// `loaded`, which also contains packages discovered in source files,
    /// so removing a `library()` call during an incremental edit removes
    /// that package from the next project-wide union.
    declared_loaded: HashSet<String>,
    /// Per-file bare-name search paths.  Kept apart from `loaded`, whose
    /// project-wide union is intentionally used for dplyr NSE gating.
    bare_loaded: HashMap<String, HashSet<String>>,
    /// Names supplied by project metadata rather than R assignments.
    /// R package `NAMESPACE` imports are the primary source: an
    /// `importFrom(shiny, tags)` directive proves that `tags` is bound in
    /// every package source file even when ry has no type stub for Shiny.
    /// Such bindings deliberately resolve to opaque values.
    external_bindings: HashMap<String, HashSet<String>>,
    imported_from: HashMap<String, HashMap<String, String>>,
    external_s3_methods: HashMap<String, HashSet<(String, String)>>,
    load_bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    user_stubs: Arc<BTreeMap<String, Typeshed>>,
    /// Pass-1 output cached independently for each source path. Incremental
    /// checks invalidate only the entry updated through `update_file`.
    collected_files: HashMap<String, CollectedFile>,
    /// Top-level bindings collected independently for each file, then pooled
    /// for project-wide diagnostic emission.
    file_known_vars: HashMap<String, HashSet<String>>,
    /// Paths whose content changed since the last successful emit.
    /// These must be re-emitted regardless of table changes.
    dirty_paths: HashSet<String>,
    /// Function names from invalidated pass-1 cache entries. Retaining these
    /// names lets the reverse call graph reach callers when an update removes
    /// or renames a function.
    invalidated_fns: HashSet<String>,
    /// The `loaded` set from the previous emit, used to detect project-wide
    /// invalidation (a new `library()` call changes diagnostics everywhere).
    prev_loaded: Option<HashSet<String>>,
    /// Whether `refine_and_emit` has completed at least once. Separates
    /// the first check (refine and emit everything) from incremental
    /// ones. The compared values live in `prev_fn_returns` and
    /// `prev_fn_signatures`.
    has_prev_emit: bool,
    /// Per-file set of function names called (from `call_sites`), cached
    /// so the dirty-set computation in `refine_and_emit` can check whether
    /// a file references any function whose return slot changed.
    file_called_fns: HashMap<String, HashSet<String>>,
    /// Actual callable reads during emission include callbacks and aliases
    /// absent from syntactic call sites. Names survive return-slot renumbering.
    file_read_fns: HashMap<String, HashSet<String>>,
    /// Actual per-function reads from completed refinement rounds. These only
    /// limit refinement; file emission keeps its conservative dependencies.
    refinement_dependencies: HashMap<String, HashSet<String>>,
    /// Alias-based attachments are absent from collection and can affect any body.
    refinement_discovered_attachments: bool,
    #[cfg(test)]
    last_refinement_counts: HashMap<String, usize>,
    /// Previous pass-2 refined return types, keyed by function name.
    /// Used to seed the next fixpoint iteration so already-converged
    /// entries start from their refined value rather than re-converging
    /// from scratch.
    prev_fn_returns: HashMap<String, ry_core::RType>,
    /// Previous caller-visible parameter signatures, keyed by function name.
    /// Return slots alone are insufficient: argument names, order, required
    /// status, evaluation semantics, and parameter types all affect callers.
    prev_fn_signatures: HashMap<String, CallerVisibleSignature>,
    /// Previous pooled known_vars set, used to detect when non-function
    /// bindings changed across files (affects RY010 diagnostics).
    prev_known_vars: HashSet<String>,
    /// Callable bindings without return slots also affect call resolution.
    prev_callable_vars: HashSet<String>,
    /// Escaped operator names gate refinement and emission across the project.
    prev_escaped_operator_names: bool,
    /// When true, pass-3 emitters snapshot each file's lexical scopes.
    /// Off by default; see [`Checker::enable_scope_capture`].
    capture_scopes: bool,
    capture_references: bool,
    reference_facts: Vec<(String, crate::ReferenceFacts)>,
    /// Scope records from the most recent emission, one entry per
    /// re-emitted file. Files served from the incremental cache keep no
    /// records, so a cold `check()` (which emits every file) is the
    /// complete view.
    scope_records: Vec<(String, Vec<crate::ScopeRecord>)>,
    /// Test-visible counter: how many files were actually emitted (not
    /// served from cache) in the most recent `refine_and_emit` call.
    /// Asserted on in unit tests the same way `parse_count` is in backend.rs.
    #[doc(hidden)]
    pub emit_count: usize,
}

#[derive(Clone)]
pub(crate) struct CollectedFile {
    pub(crate) fn_table: FnTable,
    pub(crate) return_slots: ReturnSlots,
    pub(crate) loaded: HashSet<String>,
}

/// Replace `current` with `new` when they differ, returning whether the
/// replacement happened. The equality-aware setters below use this to
/// skip the all-dirty invalidation an unchanged value would cause.
fn set_if_changed<T: PartialEq>(current: &mut T, new: T) -> bool {
    if *current == new {
        false
    } else {
        *current = new;
        true
    }
}

impl Project {
    /// Construct an empty project with no files and empty tables.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a parsed file to the project. Call this for every file
    /// before calling [`check`](Self::check).
    ///
    /// The order in which files are added determines top-level
    /// shadowing: if `utils.R` and `other.R` both define `f`, the file
    /// added later wins. This mirrors R's `source()` semantics, where
    /// the most recently sourced file's top-level bindings override
    /// earlier ones.
    pub fn add_file(&mut self, path: String, file: SourceFile) {
        self.add_file_arc(path, Arc::new(file));
    }

    /// Add a pre-parsed file without wrapping. Use when the caller
    /// already holds an `Arc<SourceFile>` and would otherwise deep-clone
    /// the file just to hand it to [`add_file`](Self::add_file).
    pub fn add_file_arc(&mut self, path: String, file: Arc<SourceFile>) {
        self.dirty_paths.insert(path.clone());
        self.files.push((path, file));
    }

    /// Replace an existing parsed file while preserving project order, or
    /// append it when the path is new. Only that file's pass-1 cache entry is
    /// invalidated; `check_incremental` reuses every other file's collection.
    pub fn update_file(&mut self, path: String, file: Arc<SourceFile>) {
        if let Some(previous) = self.collected_files.remove(&path) {
            self.invalidated_fns
                .extend(previous.fn_table.fns.keys().cloned());
        }
        self.file_known_vars.remove(&path);
        self.dirty_paths.insert(path.clone());
        if let Some((_, existing)) = self
            .files
            .iter_mut()
            .find(|(existing_path, _)| existing_path == &path)
        {
            *existing = file;
        } else {
            self.files.push((path, file));
        }
    }

    /// Remove a file and its cached pass-1 collection from the project.
    pub fn remove_file(&mut self, path: &str) {
        self.files.retain(|(existing, _)| existing != path);
        if let Some(previous) = self.collected_files.remove(path) {
            self.invalidated_fns
                .extend(previous.fn_table.fns.keys().cloned());
        }
        self.file_known_vars.remove(path);
        self.file_read_fns.remove(path);
        // Removing a file changes the shared function table and pooled
        // known_vars. Conservatively mark all remaining files dirty so
        // callers of the removed file's functions are re-emitted.
        self.mark_all_dirty();
    }

    /// Declare the project's loaded packages (from `ry.toml`'s
    /// `packages` key). These are unioned at `check()` time with
    /// packages attached via `library`/`require` in
    /// any file, and the union is seeded into every pass-3 emitter so
    /// the dplyr NSE gating sees a project-wide view.
    ///
    /// Equality-aware: reinstalling the declared set already in place is a
    /// no-op. The comparison is against `declared_loaded` (the input), not
    /// `loaded`, which is recomputed from it on every check pass.
    pub fn set_loaded(&mut self, loaded: std::collections::HashSet<String>) {
        if self.declared_loaded == loaded {
            return;
        }
        self.declared_loaded = loaded.clone();
        self.loaded = loaded;
        self.mark_all_dirty();
    }

    pub fn set_bare_loaded(&mut self, loaded: HashMap<String, HashSet<String>>) {
        if set_if_changed(&mut self.bare_loaded, loaded) {
            self.mark_all_dirty();
        }
    }

    /// Mark every file dirty so the next incremental check re-emits all.
    fn mark_all_dirty(&mut self) {
        let paths: Vec<String> = self.files.iter().map(|(p, _)| p.clone()).collect();
        for p in paths {
            self.dirty_paths.insert(p);
        }
    }

    /// Opt this project into snapshotting every file's lexical scopes
    /// during the next check. The records replace those of the previous
    /// emission and are read back with
    /// [`take_scope_records`](Self::take_scope_records).
    pub fn enable_scope_capture(&mut self) {
        self.capture_scopes = true;
        self.scope_records.clear();
    }

    /// Take the scope records captured by the most recent check. Empty
    /// unless [`enable_scope_capture`](Self::enable_scope_capture) was
    /// called before it.
    pub fn take_scope_records(&mut self) -> Vec<(String, Vec<crate::ScopeRecord>)> {
        std::mem::take(&mut self.scope_records)
    }

    /// Capture reference facts for every file on each subsequent check.
    /// This bypasses the diagnostic cache so the snapshot is complete.
    pub fn enable_reference_capture(&mut self) {
        self.capture_references = true;
        self.reference_facts.clear();
    }

    /// Take the reference facts from the most recent check.
    pub fn take_reference_facts(&mut self) -> Vec<(String, crate::ReferenceFacts)> {
        std::mem::take(&mut self.reference_facts)
    }

    /// Install runtime package stubs. User packages, including `base`,
    /// replace same-named embedded packages wholesale for this project.
    /// Equality-aware: installing the same `Arc` again is a no-op; a
    /// different stub set clears all cached collection and re-emits.
    pub fn set_user_stubs(&mut self, stubs: Arc<BTreeMap<String, Typeshed>>) {
        if Arc::ptr_eq(&self.user_stubs, &stubs) {
            return;
        }
        self.collected_files.clear();
        self.user_stubs = stubs;
        self.mark_all_dirty();
    }

    /// Declare per-file names provided by project metadata, such as
    /// `NAMESPACE`'s `importFrom()` directives. Per-file scoping prevents an
    /// import in one checked package from leaking into an unrelated package.
    pub fn set_external_bindings(&mut self, bindings: HashMap<String, HashSet<String>>) {
        if set_if_changed(&mut self.external_bindings, bindings) {
            self.mark_all_dirty();
        }
    }

    pub fn set_imported_from(&mut self, imports: HashMap<String, HashMap<String, String>>) {
        if set_if_changed(&mut self.imported_from, imports) {
            self.mark_all_dirty();
        }
    }

    pub fn set_external_s3_methods(&mut self, methods: HashMap<String, HashSet<(String, String)>>) {
        if set_if_changed(&mut self.external_s3_methods, methods) {
            self.mark_all_dirty();
        }
    }

    pub fn set_load_bindings(
        &mut self,
        bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    ) {
        if set_if_changed(&mut self.load_bindings, bindings) {
            self.mark_all_dirty();
        }
    }

    /// Run the three-pass check across all added files. Returns a map
    /// (as a `Vec<(path, Vec<Diagnostic>)>` preserving input order)
    /// from each file's path to the diagnostics emitted for that file.
    ///
    /// Calling `check` twice on the same `Project` is safe but
    /// wasteful: each call re-collects and re-refines from scratch.
    /// For incremental updates, use [`update_file`](Self::update_file)
    /// followed by [`check_incremental`](Self::check_incremental).
    pub fn check(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        // Pass 1: one collection walk per file. Each file's functions
        // AND its `library`/`require` attachments are harvested in the
        // same pass (issue #178); the attachments are unioned with the
        // project-declared `loaded` set (from `ry.toml`'s `packages`
        // key). The union is seeded into every pass-3 emitter so a
        // `library(dplyr)` in any file makes dplyr NSE verbs resolve
        // everywhere (matching R's source()-based cross-file semantics).
        let mut union_loaded = self.declared_loaded.clone();
        let mut fn_table = FnTable::default();
        let mut return_slots = ReturnSlots::default();
        self.file_known_vars.clear();
        for (path, file) in &self.files {
            let mut collector = Checker::new(path);
            collector.set_user_stubs(Arc::clone(&self.user_stubs));
            let loaded = collector.collect_file_fns(file);
            let (collected, slots) = collector.into_tables();
            self.file_known_vars
                .insert(path.clone(), collected.known_vars.clone());
            union_loaded.extend(loaded);
            fn_table.append_collected(&collected, &mut return_slots, &slots);
        }
        self.loaded = union_loaded.clone();
        fn_table.known_vars = self.pooled_known_vars();
        self.fn_table = fn_table;
        self.return_slots = return_slots;
        // Cold check: every file must be emitted, and prior incremental
        // state (if any) is discarded.
        self.dirty_paths = self.files.iter().map(|(p, _)| p.clone()).collect();
        self.prev_loaded = None;
        self.has_prev_emit = false;
        self.prev_fn_returns.clear();
        self.file_read_fns.clear();
        self.refinement_dependencies.clear();
        self.prev_fn_signatures.clear();
        self.prev_known_vars.clear();
        self.prev_callable_vars.clear();
        self.prev_escaped_operator_names = false;
        self.invalidated_fns.clear();
        self.refine_and_emit()
    }

    /// Check after one or more `update_file` calls, reusing pass-1
    /// collection for every unchanged file. Pass 2 still refines the merged
    /// tables to a fixpoint and pass 3 still emits every file, preserving
    /// cross-file diagnostic correctness.
    pub fn check_incremental(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        for (path, file) in &self.files {
            if self.collected_files.contains_key(path) {
                continue;
            }
            // Same combined collection walk as the cold `check`: the
            // file's functions and attachments in one pass (#178).
            let mut collector = Checker::new(path);
            collector.set_user_stubs(Arc::clone(&self.user_stubs));
            let loaded = collector.collect_file_fns(file);
            let (fn_table, return_slots) = collector.into_tables();
            self.file_known_vars
                .insert(path.clone(), fn_table.known_vars.clone());
            self.collected_files.insert(
                path.clone(),
                CollectedFile {
                    fn_table,
                    return_slots,
                    loaded,
                },
            );
        }

        let mut fn_table = FnTable::default();
        let mut return_slots = ReturnSlots::default();
        let mut loaded = self.declared_loaded.clone();
        for (path, _) in &self.files {
            let collected = self
                .collected_files
                .get(path)
                .expect("every project file has a pass-1 cache entry");
            loaded.extend(collected.loaded.iter().cloned());
            fn_table.append_collected(
                &collected.fn_table,
                &mut return_slots,
                &collected.return_slots,
            );
            // Cache the set of function names this file calls, for the
            // dirty-set computation in refine_and_emit.
            self.file_called_fns.insert(
                path.clone(),
                collected.fn_table.call_sites.keys().cloned().collect(),
            );
        }
        fn_table.known_vars = self.pooled_known_vars();
        self.fn_table = fn_table;
        self.return_slots = return_slots;
        self.loaded = loaded;
        self.refine_and_emit()
    }

    /// Compute the set of function names that need fixpoint refinement.
    ///
    /// Returns `None` when the scope is "all functions" (first call or no
    /// incremental state). Returns `Some(set)` with only the functions whose
    /// return type can have changed: dirty-file definitions and their observed
    /// callers, including forwarding and S3 metadata dependencies.
    fn compute_fixpoint_scope(&self) -> Option<HashSet<String>> {
        // First call → refine everything.
        if !self.has_prev_emit || self.refinement_discovered_attachments {
            return None;
        }
        // If loaded changed (library() calls appeared/disappeared), the stub
        // environment changed — full refinement is needed because package
        // signatures affect return types.
        if self
            .prev_loaded
            .as_ref()
            .is_some_and(|prev| prev != &self.loaded)
        {
            return None;
        }
        // A new callable can resolve a previously unknown callback or alias;
        // no observed dependency exists for that earlier lookup miss.
        if self.callable_names_changed()
            || self.prev_callable_vars != self.fn_table.callable_vars
            || self.prev_escaped_operator_names != self.fn_table.has_escaped_operator_names
        {
            return None;
        }
        // Nothing changed → nothing to refine.
        if self.dirty_paths.is_empty() {
            return Some(HashSet::new());
        }

        // Functions defined in dirty files, plus definitions removed or
        // renamed by those edits, seed the affected set.
        let mut affected = self.invalidated_fns.clone();
        for dirty_path in &self.dirty_paths {
            if let Some(collected) = self.collected_files.get(dirty_path) {
                affected.extend(collected.fn_table.fns.keys().cloned());
            }
        }

        let affected = self.with_refinement_callers(affected);

        Some(affected)
    }

    fn with_refinement_callers(&self, mut affected: HashSet<String>) -> HashSet<String> {
        let methods = crate::fixpoint::s3_evaluation_methods(&self.fn_table);
        let mut callers: HashMap<&str, HashSet<&str>> = HashMap::new();
        for (caller, dependencies) in &self.refinement_dependencies {
            for dependency in dependencies {
                callers.entry(dependency).or_default().insert(caller);
            }
        }
        // Evaluation metadata is propagated outside the body read recorder.
        // Include these edges even before any quoting/injection is present.
        for call in &self.fn_table.forwarded_calls {
            if !call.stub_callee.contains("::")
                && self.fn_table.fns.contains_key(&call.callee)
                && self.fn_table.fns.contains_key(&call.caller)
            {
                callers
                    .entry(&call.callee)
                    .or_default()
                    .insert(&call.caller);
            }
        }
        for (generic, methods) in &methods {
            for method in methods {
                callers.entry(method).or_default().insert(generic);
            }
        }
        let mut pending: Vec<_> = affected.iter().cloned().collect();
        while let Some(callee) = pending.pop() {
            for caller in callers.get(callee.as_str()).into_iter().flatten() {
                if affected.insert((*caller).to_string()) {
                    pending.push((*caller).to_string());
                }
            }
        }
        affected
    }

    fn callable_names_changed(&self) -> bool {
        self.fn_table.fns.len() != self.prev_fn_returns.len()
            || self
                .fn_table
                .fns
                .keys()
                .any(|name| !self.prev_fn_returns.contains_key(name))
    }

    fn file_depends_on(&self, path: &str, affected: &HashSet<String>) -> bool {
        self.file_called_fns
            .get(path)
            .into_iter()
            .chain(self.file_read_fns.get(path))
            .flatten()
            .any(|callee| affected.contains(callee))
    }

    /// Expand a set of changed callees through the cached reverse call graph.
    /// Syntactic calls and observed callable reads are collected per file,
    /// so every function in a dependent file is a conservative caller.
    /// Repeating to a fixpoint reaches callers in other files transitively.
    fn with_transitive_callers(&self, mut affected: HashSet<String>) -> HashSet<String> {
        let mut changed = true;
        while changed {
            changed = false;
            for (path, collected) in &self.collected_files {
                if !self.file_depends_on(path, &affected) {
                    continue;
                }
                for caller in collected.fn_table.fns.keys() {
                    changed |= affected.insert(caller.clone());
                }
            }
        }
        affected
    }

    fn refine_and_emit(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        // Pass 2: refine every function's inferred return type until
        // the shared table stabilizes. A single Checker drives the
        // fixpoint loop; its table is then handed back to the Project.
        //
        // Optimization: seed the fixpoint with the previous run's
        // refined return types (keyed by function name). Already-converged
        // entries keep their refined value, so the loop needs fewer
        // iterations to re-stabilize after a small edit.
        // Compute scope before moving the current tables into the refiner;
        // S3 generic-to-method dependencies are recorded in `fn_table`.
        let mut fixpoint_scope = self.compute_fixpoint_scope();
        let mut refiner = Checker::with_tables(
            "__project_pass2__",
            std::mem::take(&mut self.fn_table),
            std::mem::take(&mut self.return_slots),
        );
        refiner.set_loaded(self.loaded.clone());
        refiner.set_user_stubs(Arc::clone(&self.user_stubs));
        refiner.refinement_dependencies = Some(HashMap::new());

        // Scoping: refine only functions whose return type can have
        // changed, rather than the entire project. On the first call or
        // when `loaded` changed, fall back to refining everything.
        if let Some(ref scope) = fixpoint_scope {
            refiner.seed_return_types(&self.prev_fn_returns, scope);
            refiner.seed_caller_visible_signatures(&self.prev_fn_signatures, scope);
            refiner.run_fixpoint_scoped(scope);
        } else {
            // Full invalidation starts from fresh collection, like a cold
            // check. Old metadata can belong to a replaced or shadowed
            // definition, and recursive returns can preserve an old seed.
            refiner.run_fixpoint();
        }
        if fixpoint_scope.is_some() && *refiner.loaded != self.loaded {
            // Alias-based library/require calls can change every bare lookup.
            // Rebuild from collection: widening the seeded scope could retain
            // stale recursive returns or argument evaluation metadata.
            let mut table = FnTable::default();
            let mut slots = ReturnSlots::default();
            for (path, _) in &self.files {
                let collected = &self.collected_files[path];
                table.append_collected(&collected.fn_table, &mut slots, &collected.return_slots);
            }
            table.known_vars = self.pooled_known_vars();
            #[cfg(test)]
            let attempted_counts = std::mem::take(&mut refiner.refinement_counts);
            refiner = Checker::with_tables("__project_pass2__", table, slots);
            refiner.set_loaded(self.loaded.clone());
            refiner.set_user_stubs(Arc::clone(&self.user_stubs));
            refiner.refinement_dependencies = Some(HashMap::new());
            refiner.run_fixpoint();
            #[cfg(test)]
            for (name, count) in attempted_counts {
                *refiner.refinement_counts.entry(name).or_default() += count;
            }
            fixpoint_scope = None;
        }
        self.refinement_discovered_attachments = *refiner.loaded != self.loaded;
        if fixpoint_scope.is_none() {
            self.refinement_dependencies.clear();
        }
        self.refinement_dependencies
            .extend(refiner.refinement_dependencies.take().unwrap());
        self.refinement_dependencies
            .retain(|name, _| refiner.fn_table.fns.contains_key(name));
        #[cfg(test)]
        {
            self.last_refinement_counts = std::mem::take(&mut refiner.refinement_counts);
        }
        let (fn_table, return_slots) = refiner.into_tables();
        self.fn_table = fn_table;
        self.return_slots = return_slots;

        // --- Dirty-set computation ---
        //
        // Determine which files' diagnostics can actually have changed,
        // and re-emit only those. A file must be re-emitted when:
        //
        // 1. Its own content changed (tracked in `dirty_paths`).
        // 2. The project-wide `loaded` set changed (a `library()` call
        //    appearing/disappearing in any file invalidates everything).
        // 3. Any function it calls had its inferred return type changed
        //    by pass 2 refinement.
        //
        // On the first call (no previous state), every file is emitted.
        let loaded_changed = self
            .prev_loaded
            .as_ref()
            .is_none_or(|prev| prev != &self.loaded);

        // Compute functions whose return type or complete caller-visible
        // parameter signature changed. Name-keyed snapshots avoid the historic
        // slot-index defect when functions are inserted or removed.
        let directly_changed_fns: HashSet<String> = self
            .fn_table
            .fns
            .iter()
            .filter_map(|(name, function)| {
                let current_return = self.return_slots.0.get(function.return_slot);
                let return_changed = self
                    .prev_fn_returns
                    .get(name)
                    .is_none_or(|previous| current_return != Some(previous));
                let current_signature = function.caller_visible_signature();
                let signature_changed = self
                    .prev_fn_signatures
                    .get(name)
                    .is_none_or(|previous| previous != &current_signature);
                (return_changed || signature_changed).then(|| name.clone())
            })
            .collect();
        let changed_fns = self.with_transitive_callers(
            directly_changed_fns
                .into_iter()
                .chain(self.invalidated_fns.iter().cloned())
                .collect(),
        );

        // S3/S4 methods: conservatively re-emit all when any callable state changed.
        let changed_s3: HashSet<usize> = if changed_fns.is_empty() {
            HashSet::new()
        } else {
            self.fn_table
                .s3_methods
                .values()
                .chain(self.fn_table.s4_methods.values())
                .copied()
                .collect()
        };

        // Combine: a file is dirty if it was content-changed, if loaded
        // changed at all, or if it calls any function whose return slot
        // changed. When loaded changes, every file is dirty.
        // On the first call, every file must be emitted. Otherwise, use
        // the incremental dirty set.
        let known_vars_changed = self.prev_known_vars != self.fn_table.known_vars
            || self.prev_callable_vars != self.fn_table.callable_vars
            || self.prev_escaped_operator_names != self.fn_table.has_escaped_operator_names;
        let first_call = !self.has_prev_emit;
        let must_emit: HashSet<&str> = if first_call
            || loaded_changed
            || known_vars_changed
            || self.callable_names_changed()
        {
            self.files.iter().map(|(p, _)| p.as_str()).collect()
        } else {
            let mut dirty: HashSet<&str> = self.dirty_paths.iter().map(|s| s.as_str()).collect();
            for (path, _) in &self.files {
                if dirty.contains(path.as_str()) {
                    continue;
                }
                // Does this file call any function whose return type changed?
                if self.file_depends_on(path, &changed_fns) {
                    dirty.insert(path.as_str());
                }
                // Conservatively: if any S3/S4 method slot changed, emit
                // this file. S3 dispatch is dynamic; we cannot cheaply
                // determine which files trigger changed S3 methods.
                if !changed_s3.is_empty() {
                    dirty.insert(path.as_str());
                }
            }
            dirty
        };

        // Pass 3: per-file diagnostic emission. Each file gets a fresh
        // Checker that SHARES the refined tables via an `Arc` handle --
        // pass 3 is read-only on the tables (every mutation site is in
        // passes 1/2), so only the refcount is bumped per file, not the
        // tables themselves.
        //
        // Optimization: only emit files in the dirty set. Files not in
        // the set keep their previously-emitted diagnostics unchanged.
        let fn_table = Arc::new(std::mem::take(&mut self.fn_table));
        let package_known_vars = Arc::new(fn_table.known_vars.clone());
        let return_slots = Arc::new(std::mem::take(&mut self.return_slots));
        let loaded = Arc::new(std::mem::take(&mut self.loaded));
        let external_bindings = Arc::new(std::mem::take(&mut self.external_bindings));
        let imported_from = Arc::new(std::mem::take(&mut self.imported_from));
        let external_s3_methods = Arc::new(std::mem::take(&mut self.external_s3_methods));
        let load_bindings = Arc::new(std::mem::take(&mut self.load_bindings));
        let bare_loaded = Arc::new(std::mem::take(&mut self.bare_loaded));
        let user_stubs = Arc::clone(&self.user_stubs);

        // Split files into those that need emission and those that can
        // reuse cached diagnostics.
        let emit_indices: Vec<usize> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, (path, _))| self.capture_references || must_emit.contains(path.as_str()))
            .map(|(i, _)| i)
            .collect();
        self.emit_count = emit_indices.len();
        let capture_scopes = self.capture_scopes;
        let capture_references = self.capture_references;

        let mut names_by_slot: HashMap<usize, Vec<&String>> = HashMap::new();
        for (name, function) in &fn_table.fns {
            names_by_slot
                .entry(function.return_slot)
                .or_default()
                .push(name);
        }
        let per_file: Vec<FileEmission> = emit_indices
            .par_iter()
            .map(|&i| {
                let (path, file) = &self.files[i];
                let mut emitter = Checker::with_shared_tables(
                    path,
                    Arc::clone(&fn_table),
                    Arc::clone(&return_slots),
                );
                emitter.disable_user_call_argument_validation();
                emitter.set_shared_known_vars(Arc::clone(&package_known_vars));
                emitter.set_shared_loaded(Arc::clone(&loaded));
                emitter.set_bare_loaded(
                    bare_loaded
                        .get(path)
                        .cloned()
                        // Direct Project users only have the declared set;
                        // CLI installs precise per-file paths above.
                        .unwrap_or_else(|| loaded.as_ref().clone()),
                );
                emitter.set_user_stubs(Arc::clone(&user_stubs));
                emitter.set_external_bindings(
                    external_bindings.get(path).cloned().unwrap_or_default(),
                );
                emitter.set_imported_from(imported_from.get(path).cloned().unwrap_or_default());
                emitter.set_external_s3_methods(
                    external_s3_methods.get(path).cloned().unwrap_or_default(),
                );
                emitter.set_load_bindings(load_bindings.get(path).cloned().unwrap_or_default());
                if capture_scopes {
                    emitter.enable_scope_capture();
                }
                if capture_references {
                    emitter.enable_reference_capture();
                }
                *emitter.refinement_reads.get_mut() = Some(Vec::new());
                emitter.emit_diagnostics(file);
                let read_slots: HashSet<_> = emitter
                    .refinement_reads
                    .get_mut()
                    .take()
                    .unwrap()
                    .into_iter()
                    .collect();
                let read_fns = read_slots
                    .iter()
                    .filter_map(|slot| names_by_slot.get(slot))
                    .flatten()
                    .map(|name| (*name).clone())
                    .collect();
                let records = emitter.take_scope_records();
                let references = emitter.take_reference_facts();
                FileEmission {
                    index: i,
                    path: path.clone(),
                    diagnostics: emitter.take_diagnostics(),
                    scopes: records,
                    references,
                    read_fns,
                }
            })
            .collect();

        // Restore the tables onto the Project for the next `check()` call.
        // Every emitter above has been dropped, so the Arc refcount is 1
        // and `unwrap_or_clone` returns the owned value without cloning.
        self.fn_table = Arc::unwrap_or_clone(fn_table);
        self.return_slots = Arc::unwrap_or_clone(return_slots);
        self.loaded = Arc::unwrap_or_clone(loaded);
        self.external_bindings = Arc::unwrap_or_clone(external_bindings);
        self.imported_from = Arc::unwrap_or_clone(imported_from);
        self.external_s3_methods = Arc::unwrap_or_clone(external_s3_methods);
        self.load_bindings = Arc::unwrap_or_clone(load_bindings);
        self.bare_loaded = Arc::unwrap_or_clone(bare_loaded);

        // Merge newly-emitted diagnostics with cached diagnostics from
        // files that were not in the dirty set.
        let mut result: Vec<(String, Vec<Diagnostic>)> = Vec::with_capacity(self.files.len());

        // Scope records replace the previous emission's; files served
        // from cache contribute none (see `scope_records` on the struct).
        let mut per_file = per_file;
        for emission in &mut per_file {
            self.file_read_fns.insert(
                emission.path.clone(),
                std::mem::take(&mut emission.read_fns),
            );
        }
        if capture_scopes {
            self.scope_records = per_file
                .iter_mut()
                .map(|emission| (emission.path.clone(), std::mem::take(&mut emission.scopes)))
                .collect();
        }

        if capture_references {
            self.reference_facts = per_file
                .iter_mut()
                .map(|emission| {
                    (
                        emission.path.clone(),
                        std::mem::take(&mut emission.references),
                    )
                })
                .collect();
        }

        let mut emitted_map: HashMap<usize, (String, Vec<Diagnostic>)> = per_file
            .into_iter()
            .map(|emission| (emission.index, (emission.path, emission.diagnostics)))
            .collect();

        for (i, (path, _)) in self.files.iter().enumerate() {
            if let Some((p, d)) = emitted_map.remove(&i) {
                result.push((p, d));
            } else if let Some(idx) = self.diagnostics.iter().position(|(dp, _)| dp == path) {
                // Clone cached diagnostics (they're unchanged).
                result.push(self.diagnostics[idx].clone());
            } else {
                // No cached diagnostics and not emitted (shouldn't happen
                // after the first check, but handle gracefully).
                result.push((path.clone(), Vec::new()));
            }
        }

        // Record state for the next incremental check.
        self.prev_loaded = Some(self.loaded.clone());
        self.has_prev_emit = true;
        self.prev_known_vars = self.fn_table.known_vars.clone();
        self.prev_callable_vars = self.fn_table.callable_vars.clone();
        self.prev_escaped_operator_names = self.fn_table.has_escaped_operator_names;
        // Save refined return types keyed by function name for the next
        // fixpoint seeding.
        self.prev_fn_returns = self
            .fn_table
            .fns
            .iter()
            .map(|(name, uf)| (name.clone(), self.return_slots.get(uf.return_slot)))
            .collect();
        self.prev_fn_signatures = self
            .fn_table
            .fns
            .iter()
            .map(|(name, function)| (name.clone(), function.caller_visible_signature()))
            .collect();
        self.dirty_paths.clear();
        self.invalidated_fns.clear();

        self.diagnostics = result.clone();
        result
    }

    fn pooled_known_vars(&self) -> HashSet<String> {
        self.file_known_vars
            .values()
            .flat_map(|known_vars| known_vars.iter().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::parse_file;

    fn assert_matches_cold(project: &mut Project) {
        let actual = project.check_incremental();
        let mut cold = Project::new();
        for (path, file) in &project.files {
            cold.add_file(path.clone(), (**file).clone());
        }
        let expected = cold.check();
        assert_eq!(project.prev_fn_returns, cold.prev_fn_returns);
        assert_eq!(project.prev_fn_signatures, cold.prev_fn_signatures);
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }

    #[test]
    fn function_dependencies_skip_unrelated_callers_and_retain_edges() {
        let mut project = Project::new();
        project.add_file(
            "leaf.R".into(),
            parse_file("leaf.R", "leaf <- function() 1L"),
        );
        let mut callers = "caller <- function() leaf()\n".to_string();
        for i in 0..100 {
            callers.push_str(&format!("stable{i} <- function() {i}L\n"));
        }
        project.add_file("callers.R".into(), parse_file("callers.R", &callers));
        project.add_file(
            "outer.R".into(),
            parse_file("outer.R", "outer <- function() stable0()"),
        );
        project.check();
        for source in ["leaf <- function() 'changed'", "leaf <- function() 1L"] {
            project.update_file("leaf.R".into(), parse_file("leaf.R", source).into());
            assert_matches_cold(&mut project);
            assert!(project.last_refinement_counts.contains_key("leaf"));
            assert!(project.last_refinement_counts.contains_key("caller"));
            assert_eq!(
                project.last_refinement_counts.len(),
                2,
                "{:?}",
                project.last_refinement_counts
            );
            assert!(project.refinement_dependencies["outer"].contains("stable0"));
        }
        project.update_file(
            "callers.R".into(),
            parse_file(
                "callers.R",
                &callers.replace("stable0 <- function() 0L", "stable0 <- function() 'new'"),
            )
            .into(),
        );
        assert_matches_cold(&mut project);
        assert!(project.last_refinement_counts.contains_key("outer"));
    }

    #[test]
    fn function_dependencies_cover_aliases_callbacks_and_metadata() {
        for (before, after, caller) in [
            (
                "leaf <- function() 1L",
                "leaf <- function() 'x'",
                "caller <- function() { alias <- leaf; alias() }",
            ),
            (
                "leaf <- function(x) 1L",
                "leaf <- function(x) 'x'",
                "caller <- function() lapply(1L, leaf)",
            ),
            (
                "leaf <- function(x) x",
                "leaf <- function(x) substitute(x)",
                "middle <- function(x) leaf(x)\ncaller <- function(x) middle(x)",
            ),
            (
                "generic.foo <- function(x, y) y",
                "generic.foo <- function(x, y) substitute(y)",
                "generic <- function(x, y) UseMethod('generic')\ncaller <- function(x, y) generic(x, y)",
            ),
            (
                "leaf <- function() 1L",
                "leaf <- function() other()",
                "other <- function() leaf()\ncaller <- function() other()",
            ),
            (
                "leaf <- function() 1L",
                "leaf <- function() 1L\nmissing <- function(x) 'x'",
                "caller <- function() lapply(1L, missing)",
            ),
        ] {
            let mut project = Project::new();
            project.add_file("leaf.R".into(), parse_file("leaf.R", before));
            project.add_file("caller.R".into(), parse_file("caller.R", caller));
            project.check();
            for source in [after, before, after] {
                project.update_file("leaf.R".into(), parse_file("leaf.R", source).into());
                assert_matches_cold(&mut project);
            }
        }
    }

    #[test]
    fn function_dependencies_invalidate_changed_callable_bindings() {
        let before = "list <- S7::new_class('Thing')\nz <- function() 1L";
        let after = "list <- NULL\nz <- function() 1L";
        for body in ["list(1, 2)", "list(1, 2); NULL"] {
            let callers = format!(
                "a <- function() {{ list <- 1L; {body} }}\nb <- function() a()\nc <- function() z()"
            );
            let mut project = Project::new();
            project.add_file("s7.R".into(), parse_file("s7.R", before));
            project.add_file("callers.R".into(), parse_file("callers.R", &callers));
            project.add_file(
                "constant.R".into(),
                parse_file(
                    "constant.R",
                    "stable_return <- function() { list <- 1L; list(1, 2); NULL }",
                ),
            );
            project.add_file(
                "top.R".into(),
                parse_file("top.R", "list <- 1L; list(1, 2)"),
            );
            project.check();
            for source in [after, before, after] {
                project.update_file("s7.R".into(), parse_file("s7.R", source).into());
                assert_matches_cold(&mut project);
            }
        }
    }

    #[test]
    fn function_dependencies_include_divergence_probe_helpers() {
        let before = "leaf <- function() 1L";
        let after = "leaf <- function() stop('halt')";
        let source = "middle <- function() { if (FALSE) leaf() else stop('halt') }\ncaller <- function(x) { if (!is.numeric(x)) middle(); x }";
        let mut project = Project::new();
        project.add_file("leaf.R".into(), parse_file("leaf.R", before));
        project.add_file("callers.R".into(), parse_file("callers.R", source));
        project.check();
        let previous = project.prev_fn_returns["caller"].clone();
        assert!(project.refinement_dependencies["middle"].contains("leaf"));
        for source in [after, before] {
            project.update_file("leaf.R".into(), parse_file("leaf.R", source).into());
            assert_matches_cold(&mut project);
            assert!(project.last_refinement_counts.contains_key("caller"));
            if source == after {
                assert_ne!(project.prev_fn_returns["caller"], previous);
            }
        }
    }

    #[test]
    fn scoped_refinement_restarts_after_alias_attachment() {
        let before = "a_attach <- function() NULL";
        let after = "a_attach <- function() { loader <- library; loader(dplyr); NULL }";
        let callers = "z_consumer <- function() a_attach()\nb_unrelated <- function() filter(data.frame(x = 1L), x > 0)";
        let mut warm = Project::new();
        warm.add_file("loader.R".into(), parse_file("loader.R", before));
        warm.add_file("callers.R".into(), parse_file("callers.R", callers));
        warm.check();
        let previous = warm.prev_fn_returns.clone();
        warm.update_file("loader.R".into(), parse_file("loader.R", after).into());
        let warm_diags = warm.check_incremental();
        let mut cold = Project::new();
        cold.add_file("loader.R".into(), parse_file("loader.R", after));
        cold.add_file("callers.R".into(), parse_file("callers.R", callers));
        let cold_diags = cold.check();
        assert_ne!(previous["b_unrelated"], cold.prev_fn_returns["b_unrelated"]);
        assert_eq!(warm.prev_fn_returns, cold.prev_fn_returns);
        assert_eq!(warm.prev_fn_signatures, cold.prev_fn_signatures);
        assert_eq!(format!("{warm_diags:?}"), format!("{cold_diags:?}"));
        assert!(warm.refinement_dependencies.contains_key("b_unrelated"));
        // A later edit must rediscover the alias attachment even when the
        // loader is not in that edit's dependency closure.
        warm.update_file(
            "callers.R".into(),
            parse_file("callers.R", &callers.replace("1L", "2L")).into(),
        );
        assert_matches_cold(&mut warm);
        warm.update_file("loader.R".into(), parse_file("loader.R", before).into());
        assert_matches_cold(&mut warm);
        assert!(!warm.refinement_discovered_attachments);
    }

    #[test]
    fn empty_project_has_no_diagnostics() {
        let mut project = Project::new();
        let diags = project.check();
        assert!(diags.is_empty(), "empty project should have no diags");
    }

    #[test]
    fn single_file_via_project_matches_checker() {
        // Sanity: a single-file Project should behave like a single-file
        // Checker (no surprises from the extra plumbing).
        let src = "f <- function() { \"hello\" }\ny <- f() + 1L\n";
        let file = parse_file("a.R", src);

        let mut project = Project::new();
        project.add_file("a.R".to_string(), file);
        let diags = project.check();
        let all: Vec<_> = diags.into_iter().flat_map(|(_, d)| d).collect();
        assert!(
            all.iter().any(|d| d.code == "RY040"),
            "expected RY040 from char fn + int, got {:?}",
            all
        );
    }

    #[test]
    fn shiny_fragment_paths_bind_server_ambient_names() {
        let mut project = Project::new();
        project.add_file(
            "inst/shiny/src/server/fragment.R".to_string(),
            parse_file(
                "inst/shiny/src/server/fragment.R",
                "output$value <- input$value\nsession$sendCustomMessage('x', list())\n",
            ),
        );
        let diagnostics: Vec<_> = project
            .check()
            .into_iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .collect();
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "Shiny fragments must receive input/output/session: {diagnostics:?}"
        );
    }

    #[test]
    fn loaded_package_eval_metadata_applies_to_project_functions() {
        let mut project = Project::new();
        project.add_file(
            "function.R".to_string(),
            parse_file("function.R", "list.map <- function(.data, expr) expr\n"),
        );
        project.add_file(
            "call.R".to_string(),
            parse_file("call.R", "r <- list.map(some_list(), . + score)\n"),
        );
        project.set_loaded(std::collections::HashSet::from(["rlist".to_string()]));
        let diagnostics: Vec<_> = project
            .check()
            .into_iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .collect();
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY010"),
            "project calls should honor loaded stub eval metadata: {diagnostics:?}"
        );
    }

    #[test]
    fn scope_capture_records_top_and_function_scopes_once() {
        use crate::ScopeRecordKind;

        // A nested closure: the inner body references `base`, which only
        // the outer scope binds, so the inner snapshot must still contain
        // it (R's lexical capture) while the outer records it as a local.
        // The trailing `outer()` omits both formals, which is the call
        // evidence that lets ry commit to `x`'s default type; without a
        // call site a defaulted formal stays opaque by design.
        let src = "base <- 2L\nouter <- function(x = 1L, y) {\n  local <- x + base\n  inner <- function(z) z + base\n  inner(y)\n}\nouter()\n";
        let mut project = Project::new();
        project.add_file("a.R".to_string(), parse_file("a.R", src));
        project.enable_scope_capture();
        project.check();
        let records = project.take_scope_records();
        assert_eq!(records.len(), 1, "one file: {records:?}");
        let (path, records) = &records[0];
        assert_eq!(path, "a.R");

        let mut sorted = records.clone();
        sorted.sort_by_key(|record| record.span.start);
        // top, outer, inner -- exactly one record each (the fixpoint and
        // signature walks must not double-capture).
        assert_eq!(sorted.len(), 3, "{sorted:?}");
        assert_eq!(sorted[0].kind, ScopeRecordKind::Top);
        assert!(sorted[0].name.is_none());
        assert_eq!(sorted[1].kind, ScopeRecordKind::Function);
        assert_eq!(sorted[1].name.as_deref(), Some("outer"));
        assert_eq!(
            sorted[1]
                .params
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["x", "y"]
        );
        assert_eq!(sorted[2].name.as_deref(), Some("inner"));

        let outer = &sorted[1];
        // The default-valued parameter carries its literal type; the
        // default-less one stays opaque.
        assert_eq!(
            outer.scope.get("x").map(|t| t.to_string()),
            Some("integer<len=1>".to_string())
        );
        assert!(outer.scope.parameter_bindings.contains("x"));
        // Captured from the outer scope's cloned table.
        assert!(sorted[2].scope.get("base").is_some());
        // Local assignment present in the final snapshot.
        assert!(outer.scope.get("local").is_some());
        // No capture without opting in.
        let mut plain = Project::new();
        plain.add_file("a.R".to_string(), parse_file("a.R", src));
        plain.check();
        assert!(plain.take_scope_records().is_empty());
    }
}
