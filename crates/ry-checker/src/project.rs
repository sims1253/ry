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
    apply_filter_to_diagnostics, Checker, Diagnostic, FnTable, ReturnSlots, SeverityFilter,
};
use rayon::prelude::*;
use ry_core::SourceFile;
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
    /// `check()` time with packages loaded via `library`/`require`/
    /// `requireNamespace` in any file. Seeded into every pass-3 emitter
    /// so the dplyr NSE gating sees a project-wide view.
    loaded: std::collections::HashSet<String>,
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

    /// Declare the project's loaded packages (from `ry.toml`'s
    /// `packages` key). These are unioned at `check()` time with
    /// packages loaded via `library`/`require`/`requireNamespace` in
    /// any file, and the union is seeded into every pass-3 emitter so
    /// the dplyr NSE gating sees a project-wide view.
    pub fn set_loaded(&mut self, loaded: std::collections::HashSet<String>) {
        self.loaded = loaded;
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
    /// For incremental updates, construct a fresh `Project`.
    pub fn check(&mut self) -> Vec<(String, Vec<Diagnostic>)> {
        // Pre-scan: collect packages loaded via `library`/`require`/
        // `requireNamespace` from every file and union them with the
        // project-declared `loaded` set (from `ry.toml`'s `packages`
        // key). The union is seeded into every pass-3 emitter so a
        // `library(dplyr)` in any file makes dplyr NSE verbs resolve
        // everywhere (matching R's source()-based cross-file semantics).
        // A throwaway Checker in discarding mode drives the walk; no
        // diagnostics are emitted.
        let mut union_loaded = std::mem::take(&mut self.loaded);
        let mut loaded_scanner = Checker::new("__project_loaded__");
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
        for (_path, file) in &self.files {
            collector.collect_file_fns(file);
        }
        let (fn_table, return_slots) = collector.into_tables();
        self.fn_table = fn_table;
        self.return_slots = return_slots;

        // Pass 2: refine every function's inferred return type until
        // the shared table stabilizes. A single Checker drives the
        // fixpoint loop; its table is then handed back to the Project.
        let mut refiner = Checker::with_tables(
            "__project_pass2__",
            std::mem::take(&mut self.fn_table),
            std::mem::take(&mut self.return_slots),
        );
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
                emitter.set_loaded((*loaded).clone());
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
}
