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
    files: Vec<(String, SourceFile)>,
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
}

#[derive(Clone)]
struct CollectedFile {
    fn_table: FnTable,
    return_slots: ReturnSlots,
    loaded: HashSet<String>,
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
            external_bindings: HashMap::new(),
            imported_from: HashMap::new(),
            external_s3_methods: HashMap::new(),
            load_bindings: HashMap::new(),
            user_stubs: Arc::new(BTreeMap::new()),
            collected_files: HashMap::new(),
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
        self.files.push((path, file));
    }

    /// Replace an existing parsed file while preserving project order, or
    /// append it when the path is new. Only that file's pass-1 cache entry is
    /// invalidated; `check_incremental` reuses every other file's collection.
    pub fn update_file(&mut self, path: String, file: SourceFile) {
        self.collected_files.remove(&path);
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
    }

    /// Declare the project's loaded packages (from `ry.toml`'s
    /// `packages` key). These are unioned at `check()` time with
    /// packages attached via `library`/`require` in
    /// any file, and the union is seeded into every pass-3 emitter so
    /// the dplyr NSE gating sees a project-wide view.
    pub fn set_loaded(&mut self, loaded: std::collections::HashSet<String>) {
        self.declared_loaded = loaded.clone();
        self.loaded = loaded;
    }

    /// Install runtime package stubs. User packages, including `base`,
    /// replace same-named embedded packages wholesale for this project.
    pub fn set_user_stubs(&mut self, stubs: Arc<BTreeMap<String, Typeshed>>) {
        if !Arc::ptr_eq(&self.user_stubs, &stubs) {
            self.collected_files.clear();
        }
        self.user_stubs = stubs;
    }

    /// Declare per-file names provided by project metadata, such as
    /// `NAMESPACE`'s `importFrom()` directives. Per-file scoping prevents an
    /// import in one checked package from leaking into an unrelated package.
    pub fn set_external_bindings(&mut self, bindings: HashMap<String, HashSet<String>>) {
        self.external_bindings = bindings;
    }

    pub fn set_imported_from(&mut self, imports: HashMap<String, HashMap<String, String>>) {
        self.imported_from = imports;
    }

    pub fn set_external_s3_methods(&mut self, methods: HashMap<String, HashSet<(String, String)>>) {
        self.external_s3_methods = methods;
    }

    pub fn set_load_bindings(
        &mut self,
        bindings: HashMap<String, HashMap<usize, HashSet<String>>>,
    ) {
        self.load_bindings = bindings;
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

        // Pass 1: walk every file's top-level statements, collecting
        // function definitions (and S3 method registrations) into the
        // shared FnTable. We use a throwaway Checker to drive
        // `collect_fns` and then move its populated tables back onto
        // this Project.
        let mut collector = Checker::new("__project_pass1__");
        collector.set_user_stubs(Arc::clone(&self.user_stubs));
        for (_path, file) in &self.files {
            collector.collect_file_fns(file);
        }
        let (fn_table, return_slots) = collector.into_tables();
        self.fn_table = fn_table;
        self.return_slots = return_slots;
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
                collected.fn_table.clone(),
                &mut return_slots,
                collected.return_slots.clone(),
            );
        }
        self.fn_table = fn_table;
        self.return_slots = return_slots;
        self.loaded = loaded;
        self.refine_and_emit()
    }

    fn refine_and_emit(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        // Pass 2: refine every function's inferred return type until
        // the shared table stabilizes. A single Checker drives the
        // fixpoint loop; its table is then handed back to the Project.
        let mut refiner = Checker::with_tables(
            "__project_pass2__",
            std::mem::take(&mut self.fn_table),
            std::mem::take(&mut self.return_slots),
        );
        refiner.set_user_stubs(Arc::clone(&self.user_stubs));
        refiner.run_fixpoint();
        let (fn_table, return_slots) = refiner.into_tables();
        self.fn_table = fn_table;
        self.return_slots = return_slots;

        // Pass 3: per-file diagnostic emission. Each file gets a fresh
        // Checker that SHARES the refined tables via an `Arc` handle --
        // pass 3 is read-only on the tables (every mutation site is in
        // passes 1/2), so only the refcount is bumped per file, not the
        // tables themselves.
        //
        // The emission loop is embarrassingly parallel: each file's
        // Checker is independent and the Arc-shared tables are read-
        // only, so we rayon-`par_iter` it. Diagnostics
        // come back in arbitrary thread order; we re-sort to match the
        // input file order so callers see a stable, deterministic vec.
        let fn_table = Arc::new(std::mem::take(&mut self.fn_table));
        let return_slots = Arc::new(std::mem::take(&mut self.return_slots));
        let loaded = Arc::new(std::mem::take(&mut self.loaded));
        let external_bindings = Arc::new(std::mem::take(&mut self.external_bindings));
        let imported_from = Arc::new(std::mem::take(&mut self.imported_from));
        let external_s3_methods = Arc::new(std::mem::take(&mut self.external_s3_methods));
        let load_bindings = Arc::new(std::mem::take(&mut self.load_bindings));
        let user_stubs = Arc::clone(&self.user_stubs);
        let mut per_file: Vec<(usize, String, Vec<Diagnostic>)> = self
            .files
            .par_iter()
            .enumerate()
            .map(|(i, (path, file))| {
                let mut emitter = Checker::with_shared_tables(
                    path,
                    Arc::clone(&fn_table),
                    Arc::clone(&return_slots),
                );
                emitter.disable_user_call_argument_validation();
                emitter.set_shared_loaded(Arc::clone(&loaded));
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
        // Re-sort to input file order and drop the sort index.
        per_file.sort_by_key(|(i, _, _)| *i);
        let per_file: Vec<(String, Vec<Diagnostic>)> =
            per_file.into_iter().map(|(_, p, d)| (p, d)).collect();

        self.diagnostics = per_file.clone();
        per_file
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
