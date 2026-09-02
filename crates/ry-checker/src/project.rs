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

use crate::infer::semantic_argument_name;
use crate::{
    CallerVisibleSignature, Checker, Diagnostic, FnTable, ReturnSlots, usemethod_generic_name,
};
use rayon::prelude::*;
use ry_core::SourceFile;
use ry_typeshed::Typeshed;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// A multi-file R project. Functions defined in any file are visible
/// to all other files. The fixpoint loop refines returns across the
/// whole project at once.
///
/// Files are checked in the order they were added with [`Project::add_file`].
/// That ordering matters for shadowing semantics: if two files define
/// a top-level function with the same name, the later `add_file` wins
/// (matching R's own `source()` ordering, where the most recently
/// sourced file's bindings override earlier ones).
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
    /// When true, pass-3 emitters snapshot each file's lexical scopes.
    /// Off by default; see [`Checker::enable_scope_capture`].
    capture_scopes: bool,
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

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
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
        Self {
            fn_table: FnTable::default(),
            return_slots: ReturnSlots::default(),
            files: Vec::new(),
            diagnostics: Vec::new(),
            loaded: std::collections::HashSet::new(),
            declared_loaded: HashSet::new(),
            bare_loaded: HashMap::new(),
            external_bindings: HashMap::new(),
            imported_from: HashMap::new(),
            external_s3_methods: HashMap::new(),
            load_bindings: HashMap::new(),
            user_stubs: Arc::new(BTreeMap::new()),
            collected_files: HashMap::new(),
            file_known_vars: HashMap::new(),
            dirty_paths: HashSet::new(),
            invalidated_fns: HashSet::new(),
            prev_loaded: None,
            has_prev_emit: false,
            file_called_fns: HashMap::new(),
            prev_fn_returns: HashMap::new(),
            prev_fn_signatures: HashMap::new(),
            prev_known_vars: HashSet::new(),
            capture_scopes: false,
            scope_records: Vec::new(),
            emit_count: 0,
        }
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
        self.dirty_paths.insert(path.clone());
        self.files.push((path, Arc::new(file)));
    }

    /// Add a pre-parsed file without wrapping. Use when the caller
    /// already holds an `Arc<SourceFile>` (e.g. the LSP server, which
    /// shares parse results across features).
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
        // Pre-scan: collect packages attached via `library`/`require`
        // from every file and union them with the
        // project-declared `loaded` set (from `ry.toml`'s `packages`
        // key). The union is seeded into every pass-3 emitter so a
        // `library(dplyr)` in any file makes dplyr NSE verbs resolve
        // everywhere (matching R's source()-based cross-file semantics).
        // A throwaway Checker in discarding mode drives the walk; no
        // diagnostics are emitted.
        let mut union_loaded = self.declared_loaded.clone();
        let mut loaded_scanner = Checker::new("__project_loaded__");
        loaded_scanner.set_user_stubs(Arc::clone(&self.user_stubs));
        for (_path, file) in &self.files {
            union_loaded.extend(loaded_scanner.collect_file_loaded(file));
        }
        self.loaded = union_loaded.clone();

        // Pass 1: collect each file separately before merging. Their binding
        // sets are pooled for diagnostic emission, matching source()-based
        // project semantics (including testthat helpers and examples).
        let mut fn_table = FnTable::default();
        let mut return_slots = ReturnSlots::default();
        self.file_known_vars.clear();
        for (path, file) in &self.files {
            let mut collector = Checker::new(path);
            collector.set_user_stubs(Arc::clone(&self.user_stubs));
            collector.collect_file_fns(file);
            let (collected, slots) = collector.into_tables();
            self.file_known_vars
                .insert(path.clone(), collected.known_vars.clone());
            fn_table.append_collected(&collected, &mut return_slots, &slots);
        }
        fn_table.known_vars = self.pooled_known_vars();
        self.fn_table = fn_table;
        self.return_slots = return_slots;
        // Cold check: every file must be emitted, and prior incremental
        // state (if any) is discarded.
        self.dirty_paths = self.files.iter().map(|(p, _)| p.clone()).collect();
        self.prev_loaded = None;
        self.has_prev_emit = false;
        self.prev_fn_returns.clear();
        self.prev_fn_signatures.clear();
        self.prev_known_vars.clear();
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
            let mut loaded_scanner = Checker::new(path);
            loaded_scanner.set_user_stubs(Arc::clone(&self.user_stubs));
            let loaded = loaded_scanner.collect_file_loaded(file);

            let mut collector = Checker::new(path);
            collector.set_user_stubs(Arc::clone(&self.user_stubs));
            collector.collect_file_fns(file);
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
    /// return type can have changed: those defined in dirty files plus their
    /// transitive callers via the reverse call graph.
    fn compute_fixpoint_scope(&self) -> Option<HashSet<String>> {
        // First call → refine everything.
        if !self.has_prev_emit {
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

        // S3 dispatch is not an ordinary call-graph edge: a method change can
        // change the generic's propagated quoting metadata. Include matching
        // UseMethod generics before walking their ordinary callers.
        let affected_generics: Vec<String> = self
            .fn_table
            .fns
            .iter()
            .filter_map(|(name, function)| {
                let dispatch = usemethod_generic_name(&function.body)?;
                if semantic_argument_name(name) != dispatch {
                    return None;
                }
                let prefix = format!("{dispatch}.");
                affected
                    .iter()
                    .any(|method| {
                        semantic_argument_name(method)
                            .strip_prefix(&prefix)
                            .is_some_and(|class| !class.is_empty())
                    })
                    .then(|| name.clone())
            })
            .collect();
        affected.extend(affected_generics);

        // Transitive closure: if function G's defining file calls function F,
        // and F is affected, then G is also affected.
        let affected = self.with_transitive_callers(affected);

        Some(affected)
    }

    /// Expand a set of changed callees through the cached reverse call graph.
    /// `FnTable::call_sites` is collected per file, so every function defined
    /// in a file that calls an affected function is a conservative caller.
    /// Repeating to a fixpoint reaches callers in other files transitively.
    fn with_transitive_callers(&self, mut affected: HashSet<String>) -> HashSet<String> {
        let mut changed = true;
        while changed {
            changed = false;
            for (path, collected) in &self.collected_files {
                let Some(calls) = self.file_called_fns.get(path) else {
                    continue;
                };
                if !calls.iter().any(|callee| affected.contains(callee)) {
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
        let fixpoint_scope = self.compute_fixpoint_scope();
        let mut refiner = Checker::with_tables(
            "__project_pass2__",
            std::mem::take(&mut self.fn_table),
            std::mem::take(&mut self.return_slots),
        );
        refiner.set_user_stubs(Arc::clone(&self.user_stubs));
        refiner.seed_return_types(&self.prev_fn_returns);

        // Scoping: refine only functions whose return type can have
        // changed, rather than the entire project. On the first call or
        // when `loaded` changed, fall back to refining everything.
        refiner.seed_caller_visible_signatures(&self.prev_fn_signatures, fixpoint_scope.as_ref());
        if let Some(ref scope) = fixpoint_scope {
            refiner.run_fixpoint_scoped(scope);
        } else {
            refiner.run_fixpoint();
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
        let known_vars_changed = self.prev_known_vars != self.fn_table.known_vars;
        let first_call = !self.has_prev_emit;
        let must_emit: HashSet<&str> = if first_call || loaded_changed || known_vars_changed {
            self.files.iter().map(|(p, _)| p.as_str()).collect()
        } else {
            let mut dirty: HashSet<&str> = self.dirty_paths.iter().map(|s| s.as_str()).collect();
            for (path, _) in &self.files {
                if dirty.contains(path.as_str()) {
                    continue;
                }
                // Does this file call any function whose return type changed?
                if let Some(called) = self.file_called_fns.get(path) {
                    if called
                        .iter()
                        .any(|name| changed_fns.contains(name.as_str()))
                    {
                        dirty.insert(path.as_str());
                    }
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
            .filter(|(_, (path, _))| must_emit.contains(path.as_str()))
            .map(|(i, _)| i)
            .collect();
        self.emit_count = emit_indices.len();
        let capture_scopes = self.capture_scopes;

        let per_file: Vec<(usize, String, Vec<Diagnostic>, Vec<crate::ScopeRecord>)> = emit_indices
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
                emitter.emit_diagnostics(file);
                let records = emitter.take_scope_records();
                (i, path.clone(), emitter.take_diagnostics(), records)
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
        if capture_scopes {
            self.scope_records = per_file
                .iter_mut()
                .map(|(_, path, _, records)| (path.clone(), std::mem::take(records)))
                .collect();
        }

        let mut emitted_map: HashMap<usize, (String, Vec<Diagnostic>)> = per_file
            .into_iter()
            .map(|(i, p, d, _)| (i, (p, d)))
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

/// File classification — re-exported from ry-workspace.
pub use ry_workspace::{PackageFileKind, package_file_kind};

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::RParser;

    fn parse(path: &str, src: &str) -> SourceFile {
        let mut p = RParser::new().unwrap();
        p.parse(path, src).unwrap()
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
        let file = parse("a.R", src);

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
            parse(
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
            parse("function.R", "list.map <- function(.data, expr) expr\n"),
        );
        project.add_file(
            "call.R".to_string(),
            parse("call.R", "r <- list.map(some_list(), . + score)\n"),
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
        project.add_file("a.R".to_string(), parse("a.R", src));
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
        plain.add_file("a.R".to_string(), parse("a.R", src));
        plain.check();
        assert!(plain.take_scope_records().is_empty());
    }

    #[test]
    fn scope_snapshot_is_a_plain_scope() {
        // Compile-level guard: the recorded value is the ordinary `Scope`
        // type, so dump consumers can use the same accessors as the LSP.
        use crate::{Scope, ScopeRecordKind};
        let mut project = Project::new();
        project.add_file(
            "a.R".to_string(),
            parse("a.R", "f <- function(x) { y <- x\n y }\n"),
        );
        project.enable_scope_capture();
        project.check();
        let records = project.take_scope_records();
        let _: Option<&Scope> = records[0]
            .1
            .iter()
            .find(|r| r.kind == ScopeRecordKind::Function)
            .map(|r| &r.scope);
    }
}
