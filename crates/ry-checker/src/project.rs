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

use crate::{
    Checker, Diagnostic, FnTable, ReturnSlots, SeverityFilter, apply_filter_to_diagnostics,
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
    /// Kept so `apply_filter` can run after `check()` without re-parsing.
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
    /// The `loaded` set from the previous emit, used to detect project-wide
    /// invalidation (a new `library()` call changes diagnostics everywhere).
    prev_loaded: Option<HashSet<String>>,
    /// Return-type slots from the previous emit, used to detect which
    /// functions' inferred return types changed during pass 2 refinement.
    prev_return_slots: Option<Vec<ry_core::RType>>,
    /// Per-file set of function names called (from `call_sites`), cached
    /// so the dirty-set computation in `refine_and_emit` can check whether
    /// a file references any function whose return slot changed.
    file_called_fns: HashMap<String, HashSet<String>>,
    /// Previous pass-2 refined return types, keyed by function name.
    /// Used to seed the next fixpoint iteration so already-converged
    /// entries start from their refined value rather than re-converging
    /// from scratch (Plan 33 W2).
    prev_fn_returns: HashMap<String, ry_core::RType>,
    /// Previous pooled known_vars set, used to detect when non-function
    /// bindings changed across files (affects RY010 diagnostics).
    prev_known_vars: HashSet<String>,
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
            prev_loaded: None,
            prev_return_slots: None,
            file_called_fns: HashMap::new(),
            prev_fn_returns: HashMap::new(),
            prev_known_vars: HashSet::new(),
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
        self.collected_files.remove(&path);
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
        self.collected_files.remove(path);
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
    pub fn set_loaded(&mut self, loaded: std::collections::HashSet<String>) {
        self.declared_loaded = loaded.clone();
        self.loaded = loaded;
        self.mark_all_dirty();
    }

    pub fn set_bare_loaded(&mut self, loaded: HashMap<String, HashSet<String>>) {
        self.bare_loaded = loaded;

        self.mark_all_dirty();
    }

    /// Install runtime package stubs. User packages, including `base`,
    /// replace same-named embedded packages wholesale for this project.
    /// Mark every file as dirty so the next incremental check re-emits all.
    fn mark_all_dirty(&mut self) {
        let paths: Vec<String> = self.files.iter().map(|(p, _)| p.clone()).collect();
        for p in paths {
            self.dirty_paths.insert(p);
        }
    }

    pub fn set_user_stubs(&mut self, stubs: Arc<BTreeMap<String, Typeshed>>) {
        if !Arc::ptr_eq(&self.user_stubs, &stubs) {
            self.collected_files.clear();
        }
        self.user_stubs = stubs;
        self.mark_all_dirty();
    }

    /// Declare per-file names provided by project metadata, such as
    /// `NAMESPACE`'s `importFrom()` directives. Per-file scoping prevents an
    /// import in one checked package from leaking into an unrelated package.
    pub fn set_external_bindings(&mut self, bindings: HashMap<String, HashSet<String>>) {
        self.external_bindings = bindings;

        self.mark_all_dirty();
    }

    pub fn set_imported_from(&mut self, imports: HashMap<String, HashMap<String, String>>) {
        self.imported_from = imports;

        self.mark_all_dirty();
    }

    pub fn set_external_s3_methods(&mut self, methods: HashMap<String, HashSet<(String, String)>>) {
        self.external_s3_methods = methods;

        self.mark_all_dirty();
    }

    pub fn set_load_bindings(
        &mut self,
        bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    ) {
        self.load_bindings = bindings;

        self.mark_all_dirty();
    }

    /// Run the three-pass check across all added files. Returns a map
    /// (as a `Vec<(path, Vec<Diagnostic>)>` preserving input order)
    /// from each file's path to the diagnostics emitted for that file.
    ///
    /// The returned vec is also cached on the `Project` so a follow-up
    /// call to [`apply_filter`](Self::apply_filter) can adjust
    /// severities without re-checking.
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
        self.prev_return_slots = None;
        self.prev_fn_returns.clear();
        self.prev_known_vars.clear();
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
        self.prev_return_slots.as_ref()?;
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

        // Functions defined in dirty files are the seed of the affected set.
        let mut affected: HashSet<&str> = HashSet::new();
        for dirty_path in &self.dirty_paths {
            if let Some(collected) = self.collected_files.get(dirty_path) {
                for fn_name in collected.fn_table.fns.keys() {
                    affected.insert(fn_name.as_str());
                }
            }
        }

        // Transitive closure: if function G's defining file calls function F,
        // and F is affected, then G is also affected. Iterate until fixpoint.
        let mut changed = true;
        while changed {
            changed = false;
            for (path, collected) in &self.collected_files {
                let Some(calls) = self.file_called_fns.get(path) else {
                    continue;
                };
                // Does this file call any affected function?
                if !calls.iter().any(|name| affected.contains(name.as_str())) {
                    continue;
                }
                // All functions defined in this file are potential callers.
                for fn_name in collected.fn_table.fns.keys() {
                    if affected.insert(fn_name.as_str()) {
                        changed = true;
                    }
                }
            }
        }

        Some(affected.iter().map(|s| s.to_string()).collect())
    }

    fn refine_and_emit(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        // Pass 2: refine every function's inferred return type until
        // the shared table stabilizes. A single Checker drives the
        // fixpoint loop; its table is then handed back to the Project.
        //
        // W2 optimization: seed the fixpoint with the previous run's
        // refined return types (keyed by function name). Already-converged
        // entries keep their refined value, so the loop needs fewer
        // iterations to re-stabilize after a small edit.
        let mut refiner = Checker::with_tables(
            "__project_pass2__",
            std::mem::take(&mut self.fn_table),
            std::mem::take(&mut self.return_slots),
        );
        refiner.set_user_stubs(Arc::clone(&self.user_stubs));
        refiner.seed_return_types(&self.prev_fn_returns);

        // W2 scoping: refine only functions whose return type can have
        // changed, rather than the entire project. On the first call or
        // when `loaded` changed, fall back to refining everything.
        let fixpoint_scope = self.compute_fixpoint_scope();
        if let Some(ref scope) = fixpoint_scope {
            refiner.run_fixpoint_scoped(scope);
        } else {
            refiner.run_fixpoint();
        }
        let (fn_table, return_slots) = refiner.into_tables();
        self.fn_table = fn_table;
        self.return_slots = return_slots;

        // --- Dirty-set computation (Plan 33 W1) ---
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

        // Compute the set of function names whose refined return type changed
        // since the last emit. Uses name-keyed comparison via prev_fn_returns
        // (not slot indices, which shift when functions are added/removed).
        let changed_fns: HashSet<&str> = self
            .fn_table
            .fns
            .iter()
            .filter_map(|(name, uf)| {
                let current = &self.return_slots.0.get(uf.return_slot);
                let previous = self.prev_fn_returns.get(name);
                match (current, previous) {
                    (Some(cur), Some(prev)) if **cur == *prev => None,
                    _ => Some(name.as_str()),
                }
            })
            .collect();
        // S3/S4 methods: conservatively re-emit all when any return type changed.
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
        // On the first call (prev_return_slots is None), every file must be
        // emitted. Otherwise, use the incremental dirty set.
        let known_vars_changed = self.prev_known_vars != self.fn_table.known_vars;
        let first_call = self.prev_return_slots.is_none();
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
        // W1 optimization: only emit files in the dirty set. Files not in
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

        let per_file: Vec<(usize, String, Vec<Diagnostic>)> = emit_indices
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
                emitter.emit_diagnostics(file);
                (i, path.clone(), emitter.take_diagnostics())
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

        // Build a lookup from emitted results.
        let mut emitted_map: HashMap<usize, (String, Vec<Diagnostic>)> =
            per_file.into_iter().map(|(i, p, d)| (i, (p, d))).collect();

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
        self.prev_return_slots = Some(self.return_slots.0.clone());
        // Save refined return types keyed by function name for the next
        // fixpoint seeding (W2).
        self.prev_known_vars = self.fn_table.known_vars.clone();
        self.prev_fn_returns = self
            .fn_table
            .fns
            .iter()
            .map(|(name, uf)| (name.clone(), self.return_slots.get(uf.return_slot)))
            .collect();
        self.dirty_paths.clear();

        self.diagnostics = result.clone();
        result
    }

    fn pooled_known_vars(&self) -> HashSet<String> {
        self.file_known_vars
            .values()
            .flat_map(|known_vars| known_vars.iter().cloned())
            .collect()
    }

    /// Apply a severity filter to the diagnostics cached from the most
    /// recent `check()` call. If `check()` has not been called yet,
    /// this is a no-op.
    ///
    /// This mirrors `Checker::apply_filter` but operates across every
    /// file's diagnostic vec. Callers that hold their own per-file vec
    /// (e.g. the CLI, after collecting `check()`'s return value) can
    /// instead use [`apply_filter_to_diagnostics`] directly.
    pub fn apply_filter(&mut self, filter: &SeverityFilter) {
        for (_path, diags) in &mut self.diagnostics {
            apply_filter_to_diagnostics(diags, filter);
        }
    }
}

/// The role an R source file has inside its enclosing package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFileKind {
    Library,
    TestCode,
    TestFixture,
    Inst,
    Other,
}

/// Classify a path relative to its nearest ancestor containing `DESCRIPTION`.
/// Testthat only sources runner files at `tests/` root and files with its
/// executable prefixes directly under `tests/testthat/`; deeper R files are
/// data consumed by tests, not code executed by the package test runner.
pub fn package_file_kind(path: &std::path::Path) -> PackageFileKind {
    let Some(root) = path
        .parent()
        .and_then(|parent| parent.ancestors().find(|p| p.join("DESCRIPTION").is_file()))
    else {
        return PackageFileKind::Other;
    };
    let Ok(relative) = path.strip_prefix(root) else {
        return PackageFileKind::Other;
    };
    let components: Vec<_> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    match components.as_slice() {
        ["R", _] => PackageFileKind::Library,
        ["inst", ..] => PackageFileKind::Inst,
        ["tests", file] if is_r_source_name(file) => PackageFileKind::TestCode,
        ["tests", "testthat", file] if is_r_source_name(file) && is_testthat_code_name(file) => {
            PackageFileKind::TestCode
        }
        ["tests", ..] => PackageFileKind::TestFixture,
        _ => PackageFileKind::Other,
    }
}

fn is_r_source_name(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "R" | "r" | "S" | "s" | "q"))
}

fn is_testthat_code_name(name: &str) -> bool {
    let stem = std::path::Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    ["test", "helper", "setup", "teardown"]
        .iter()
        .any(|prefix| stem.starts_with(prefix))
}

/// Whether a file is directly under a package's `R/` directory.
#[cfg(test)]
pub(crate) fn is_package_library_file(path: &str) -> bool {
    package_file_kind(std::path::Path::new(path)) == PackageFileKind::Library
}

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
}
